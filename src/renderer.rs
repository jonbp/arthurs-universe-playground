//! The Vulkan renderer: HDR scene pass -> blit mip chain (bloom) ->
//! composite pass (lensing, tonemap) + egui overlay on the swapchain.

use std::io::Cursor;

use anyhow::Result;
use ash::vk;

use crate::gpu_types::*;
use crate::mesh::icosphere;
use crate::paint::{PaintStore, MAX_LAYERS, PAINT_H, PAINT_W};
use crate::vkctx::{Buffer, Image, Swapchain, VkContext};

const FRAMES_IN_FLIGHT: usize = 2;
const HDR_FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;
const DEPTH_FORMAT: vk::Format = vk::Format::D32_SFLOAT;
pub const MAX_BODIES: usize = 1024;
pub const MAX_RINGS: usize = 256;
pub const MAX_PARTICLES: usize = 220_000;
pub const MAX_LINE_VERTS: usize = 600_000;
pub const MAX_SOLID_VERTS: usize = 30_000;
const LAYER_BYTES: usize = PAINT_W * PAINT_H * 4;
const UPLOADS_PER_FRAME: usize = 8;
/// The camera is never inside a body, so the far half of every sphere is hidden:
/// culling it halves the (expensive) planet shading where spheres overlap themselves.
const PLANET_CULL: vk::CullModeFlags = vk::CullModeFlags::BACK;

struct FrameRes {
    cmd: vk::CommandBuffer,
    fence: vk::Fence,
    image_available: vk::Semaphore,
    globals: Buffer,
    bodies: Buffer,
    rings: Buffer,
    particles: Buffer,
    lines: Buffer,
    solids: Buffer,
    staging: Buffer,
    scene_set: vk::DescriptorSet,
    comp_set: vk::DescriptorSet,
}

struct Mesh {
    vertices: Buffer,
    indices: Buffer,
    count: u32,
}

struct Targets {
    hdr: Image,
    hdr_attachment_view: vk::ImageView,
    depth: Image,
    scene_fb: vk::Framebuffer,
    final_fbs: Vec<vk::Framebuffer>,
}

pub struct Renderer {
    pub ctx: VkContext,
    pub swapchain: Swapchain,
    render_finished: Vec<vk::Semaphore>,
    frames: Vec<FrameRes>,
    frame_index: usize,
    cmd_pool: vk::CommandPool,
    targets: Targets,
    scene_pass: vk::RenderPass,
    final_pass: vk::RenderPass,
    scene_set_layout: vk::DescriptorSetLayout,
    comp_set_layout: vk::DescriptorSetLayout,
    scene_layout: vk::PipelineLayout,
    comp_layout: vk::PipelineLayout,
    pipelines: Vec<vk::Pipeline>,
    sky_pipe: vk::Pipeline,
    planet_pipe: vk::Pipeline,
    ring_pipe: vk::Pipeline,
    particle_pipe: vk::Pipeline,
    line_pipe: vk::Pipeline,
    solid_pipe: vk::Pipeline,
    comp_pipe: vk::Pipeline,
    desc_pool: vk::DescriptorPool,
    sphere_ultra: Mesh,
    sphere_hi: Mesh,
    sphere_lo: Mesh,
    paint: Image,
    paint_sampler: vk::Sampler,
    hdr_sampler: vk::Sampler,
    pub egui: Option<egui_ash_renderer::Renderer<egui_ash_renderer::allocator::DefaultAllocator>>,
    pub needs_resize: bool,
    /// Set to request a screenshot of the next frame (BGRA8 pixels end up in `captured`).
    pub capture: bool,
    pub captured: Option<(u32, u32, Vec<u8>)>,
    capture_buf: Option<Buffer>,
}

fn load_shader(device: &ash::Device, bytes: &[u8]) -> Result<vk::ShaderModule> {
    let code = ash::util::read_spv(&mut Cursor::new(bytes))?;
    let info = vk::ShaderModuleCreateInfo::default().code(&code);
    Ok(unsafe { device.create_shader_module(&info, None)? })
}

macro_rules! spv {
    ($name:literal) => {
        include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".spv"))
    };
}

#[derive(Clone, Copy, PartialEq)]
enum Blend {
    Opaque,
    Premultiplied,
}

struct PipeDesc<'a> {
    module: vk::ShaderModule,
    layout: vk::PipelineLayout,
    pass: vk::RenderPass,
    bindings: &'a [vk::VertexInputBindingDescription],
    attributes: &'a [vk::VertexInputAttributeDescription],
    topology: vk::PrimitiveTopology,
    blend: Blend,
    depth_test: bool,
    depth_write: bool,
    has_depth: bool,
    cull: vk::CullModeFlags,
}

fn make_pipeline(device: &ash::Device, d: &PipeDesc) -> Result<vk::Pipeline> {
    let stages = [
        vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::VERTEX).module(d.module).name(c"vs_main"),
        vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::FRAGMENT).module(d.module).name(c"fs_main"),
    ];
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(d.bindings)
        .vertex_attribute_descriptions(d.attributes);
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default().topology(d.topology);
    let viewport = vk::PipelineViewportStateCreateInfo::default().viewport_count(1).scissor_count(1);
    let raster = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(d.cull)
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let depth = vk::PipelineDepthStencilStateCreateInfo::default()
        .depth_test_enable(d.depth_test)
        .depth_write_enable(d.depth_write)
        .depth_compare_op(vk::CompareOp::GREATER_OR_EQUAL);
    let attachment = match d.blend {
        Blend::Opaque => vk::PipelineColorBlendAttachmentState::default().color_write_mask(vk::ColorComponentFlags::RGBA),
        Blend::Premultiplied => vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::ONE)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .alpha_blend_op(vk::BlendOp::ADD)
            .color_write_mask(vk::ColorComponentFlags::RGBA),
    };
    let attachments = [attachment];
    let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
    let dyn_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dyn_states);
    let mut info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport)
        .rasterization_state(&raster)
        .multisample_state(&multisample)
        .color_blend_state(&blend)
        .dynamic_state(&dynamic)
        .layout(d.layout)
        .render_pass(d.pass)
        .subpass(0);
    if d.has_depth {
        info = info.depth_stencil_state(&depth);
    }
    let pipes = unsafe { device.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None) }.map_err(|(_, e)| e)?;
    Ok(pipes[0])
}

impl Renderer {
    pub fn new(ctx: VkContext, width: u32, height: u32) -> Result<Renderer> {
        let device = ctx.device.clone();
        let swapchain = Swapchain::new(&ctx, width, height, vk::SwapchainKHR::null())?;

        let cmd_pool = unsafe {
            device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(ctx.queue_family)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )?
        };

        // ---- render passes ----
        let scene_pass = {
            let attachments = [
                vk::AttachmentDescription::default()
                    .format(HDR_FORMAT)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .load_op(vk::AttachmentLoadOp::CLEAR)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .initial_layout(vk::ImageLayout::UNDEFINED)
                    .final_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL),
                vk::AttachmentDescription::default()
                    .format(DEPTH_FORMAT)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .load_op(vk::AttachmentLoadOp::CLEAR)
                    .store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .initial_layout(vk::ImageLayout::UNDEFINED)
                    .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL),
            ];
            let color_ref = [vk::AttachmentReference { attachment: 0, layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL }];
            let depth_ref = vk::AttachmentReference { attachment: 1, layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL };
            let subpass = [vk::SubpassDescription::default()
                .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                .color_attachments(&color_ref)
                .depth_stencil_attachment(&depth_ref)];
            let deps = [
                vk::SubpassDependency::default()
                    .src_subpass(vk::SUBPASS_EXTERNAL)
                    .dst_subpass(0)
                    .src_stage_mask(
                        vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                            | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
                            | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS
                            | vk::PipelineStageFlags::FRAGMENT_SHADER
                            | vk::PipelineStageFlags::TRANSFER,
                    )
                    .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE | vk::AccessFlags::TRANSFER_WRITE)
                    .dst_stage_mask(
                        vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                            | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
                            | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS
                            | vk::PipelineStageFlags::VERTEX_SHADER
                            | vk::PipelineStageFlags::FRAGMENT_SHADER,
                    )
                    .dst_access_mask(
                        vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                            | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE
                            | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                            | vk::AccessFlags::SHADER_READ,
                    ),
                vk::SubpassDependency::default()
                    .src_subpass(0)
                    .dst_subpass(vk::SUBPASS_EXTERNAL)
                    .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                    .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                    .dst_stage_mask(vk::PipelineStageFlags::TRANSFER)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_READ),
            ];
            let info = vk::RenderPassCreateInfo::default().attachments(&attachments).subpasses(&subpass).dependencies(&deps);
            unsafe { device.create_render_pass(&info, None)? }
        };
        let final_pass = {
            let attachments = [vk::AttachmentDescription::default()
                .format(swapchain.format)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)];
            let color_ref = [vk::AttachmentReference { attachment: 0, layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL }];
            let subpass = [vk::SubpassDescription::default().pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS).color_attachments(&color_ref)];
            let deps = [vk::SubpassDependency::default()
                .src_subpass(vk::SUBPASS_EXTERNAL)
                .dst_subpass(0)
                .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                .src_access_mask(vk::AccessFlags::empty())
                .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)];
            let info = vk::RenderPassCreateInfo::default().attachments(&attachments).subpasses(&subpass).dependencies(&deps);
            unsafe { device.create_render_pass(&info, None)? }
        };

        // ---- descriptor layouts ----
        let all = vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT;
        let scene_set_layout = {
            let b = [
                vk::DescriptorSetLayoutBinding::default().binding(0).descriptor_type(vk::DescriptorType::UNIFORM_BUFFER).descriptor_count(1).stage_flags(all),
                vk::DescriptorSetLayoutBinding::default().binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(all),
                vk::DescriptorSetLayoutBinding::default().binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(all),
                vk::DescriptorSetLayoutBinding::default().binding(3).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(all),
                vk::DescriptorSetLayoutBinding::default().binding(4).descriptor_type(vk::DescriptorType::SAMPLED_IMAGE).descriptor_count(1).stage_flags(all),
                vk::DescriptorSetLayoutBinding::default().binding(5).descriptor_type(vk::DescriptorType::SAMPLER).descriptor_count(1).stage_flags(all),
            ];
            unsafe { device.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&b), None)? }
        };
        let comp_set_layout = {
            let b = [
                vk::DescriptorSetLayoutBinding::default().binding(0).descriptor_type(vk::DescriptorType::UNIFORM_BUFFER).descriptor_count(1).stage_flags(all),
                vk::DescriptorSetLayoutBinding::default().binding(1).descriptor_type(vk::DescriptorType::SAMPLED_IMAGE).descriptor_count(1).stage_flags(all),
                vk::DescriptorSetLayoutBinding::default().binding(2).descriptor_type(vk::DescriptorType::SAMPLER).descriptor_count(1).stage_flags(all),
            ];
            unsafe { device.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&b), None)? }
        };
        let scene_layout = unsafe {
            device.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&[scene_set_layout]), None)?
        };
        let comp_layout = unsafe {
            device.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&[comp_set_layout]), None)?
        };

        // ---- pipelines ----
        let sky_mod = load_shader(&device, spv!("sky"))?;
        let planet_mod = load_shader(&device, spv!("planet"))?;
        let ring_mod = load_shader(&device, spv!("ring"))?;
        let particle_mod = load_shader(&device, spv!("particle"))?;
        let line_mod = load_shader(&device, spv!("line"))?;
        let solid_mod = load_shader(&device, spv!("solid"))?;
        let comp_mod = load_shader(&device, spv!("composite"))?;

        let base = PipeDesc {
            module: sky_mod,
            layout: scene_layout,
            pass: scene_pass,
            bindings: &[],
            attributes: &[],
            topology: vk::PrimitiveTopology::TRIANGLE_LIST,
            blend: Blend::Opaque,
            depth_test: false,
            depth_write: false,
            has_depth: true,
            cull: vk::CullModeFlags::NONE,
        };
        // The sky is drawn after the opaque geometry and depth-tested against it (it sits at
        // the far plane), so its starfield and nebula noise only run where sky is visible.
        let sky_pipe = make_pipeline(&device, &PipeDesc { depth_test: true, ..base })?;
        let sphere_bind = [vk::VertexInputBindingDescription { binding: 0, stride: 12, input_rate: vk::VertexInputRate::VERTEX }];
        let sphere_attr = [vk::VertexInputAttributeDescription { location: 0, binding: 0, format: vk::Format::R32G32B32_SFLOAT, offset: 0 }];
        let planet_pipe = make_pipeline(
            &device,
            &PipeDesc { module: planet_mod, bindings: &sphere_bind, attributes: &sphere_attr, depth_test: true, depth_write: true, cull: PLANET_CULL, ..base },
        )?;
        let ring_pipe = make_pipeline(&device, &PipeDesc { module: ring_mod, blend: Blend::Premultiplied, depth_test: true, ..base })?;
        let particle_pipe = make_pipeline(&device, &PipeDesc { module: particle_mod, blend: Blend::Premultiplied, depth_test: true, ..base })?;
        let line_bind = [vk::VertexInputBindingDescription { binding: 0, stride: 32, input_rate: vk::VertexInputRate::VERTEX }];
        let line_attr = [
            vk::VertexInputAttributeDescription { location: 0, binding: 0, format: vk::Format::R32G32B32A32_SFLOAT, offset: 0 },
            vk::VertexInputAttributeDescription { location: 1, binding: 0, format: vk::Format::R32G32B32A32_SFLOAT, offset: 16 },
        ];
        let line_pipe = make_pipeline(
            &device,
            &PipeDesc {
                module: line_mod,
                bindings: &line_bind,
                attributes: &line_attr,
                topology: vk::PrimitiveTopology::LINE_LIST,
                blend: Blend::Premultiplied,
                depth_test: true,
                ..base
            },
        )?;
        // Rockets: opaque lit triangles sharing the line vertex layout.
        let solid_pipe = make_pipeline(
            &device,
            &PipeDesc { module: solid_mod, bindings: &line_bind, attributes: &line_attr, depth_test: true, depth_write: true, ..base },
        )?;
        let comp_pipe = make_pipeline(
            &device,
            &PipeDesc { module: comp_mod, layout: comp_layout, pass: final_pass, has_depth: false, ..base },
        )?;
        unsafe {
            for m in [sky_mod, planet_mod, ring_mod, particle_mod, line_mod, solid_mod, comp_mod] {
                device.destroy_shader_module(m, None);
            }
        }

        // ---- meshes ----
        let make_mesh = |sub: u32| -> Result<Mesh> {
            let (v, i) = icosphere(sub);
            let vb = ctx.create_buffer((v.len() * 12) as u64, vk::BufferUsageFlags::VERTEX_BUFFER)?;
            vb.write(&v);
            let ib = ctx.create_buffer((i.len() * 4) as u64, vk::BufferUsageFlags::INDEX_BUFFER)?;
            ib.write(&i);
            Ok(Mesh { vertices: vb, indices: ib, count: i.len() as u32 })
        };
        let sphere_ultra = make_mesh(7)?;
        let sphere_hi = make_mesh(6)?;
        let sphere_lo = make_mesh(3)?;

        // ---- paint texture array ----
        let paint = ctx.create_image(
            PAINT_W as u32,
            PAINT_H as u32,
            1,
            MAX_LAYERS as u32,
            vk::Format::R8G8B8A8_UNORM,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            vk::ImageAspectFlags::COLOR,
        )?;
        let range = vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: MAX_LAYERS as u32,
        };
        ctx.one_time(cmd_pool, |cmd| unsafe {
            let b = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .image(paint.image)
                .subresource_range(range);
            device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[b]);
            let clear = vk::ClearColorValue { float32: [128.0 / 255.0, 0.0, 0.0, 1.0] };
            device.cmd_clear_color_image(cmd, paint.image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &clear, &[range]);
            let b = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .image(paint.image)
                .subresource_range(range);
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::VERTEX_SHADER | vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[b],
            );
        })?;

        let paint_sampler = unsafe {
            device.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .address_mode_u(vk::SamplerAddressMode::REPEAT)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                None,
            )?
        };
        let hdr_sampler = unsafe {
            device.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .max_lod(16.0),
                None,
            )?
        };

        // ---- descriptor pool & per-frame resources ----
        let n = FRAMES_IN_FLIGHT as u32;
        let sizes = [
            vk::DescriptorPoolSize { ty: vk::DescriptorType::UNIFORM_BUFFER, descriptor_count: 2 * n },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_BUFFER, descriptor_count: 3 * n },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLED_IMAGE, descriptor_count: 2 * n },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLER, descriptor_count: 2 * n },
        ];
        let desc_pool = unsafe { device.create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(2 * n).pool_sizes(&sizes), None)? };

        let targets = Self::create_targets(&ctx, &swapchain, scene_pass, final_pass)?;

        let mut frames = Vec::new();
        for _ in 0..FRAMES_IN_FLIGHT {
            let cmd = unsafe {
                device.allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default().command_pool(cmd_pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1),
                )?[0]
            };
            let fence = unsafe { device.create_fence(&vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED), None)? };
            let image_available = unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)? };
            let globals = ctx.create_buffer(std::mem::size_of::<Globals>() as u64, vk::BufferUsageFlags::UNIFORM_BUFFER)?;
            let bodies = ctx.create_buffer((MAX_BODIES * std::mem::size_of::<GpuBody>()) as u64, vk::BufferUsageFlags::STORAGE_BUFFER)?;
            let rings = ctx.create_buffer((MAX_RINGS * std::mem::size_of::<GpuRing>()) as u64, vk::BufferUsageFlags::STORAGE_BUFFER)?;
            let particles = ctx.create_buffer((MAX_PARTICLES * std::mem::size_of::<GpuParticle>()) as u64, vk::BufferUsageFlags::STORAGE_BUFFER)?;
            let lines = ctx.create_buffer((MAX_LINE_VERTS * std::mem::size_of::<LineVertex>()) as u64, vk::BufferUsageFlags::VERTEX_BUFFER)?;
            let solids = ctx.create_buffer((MAX_SOLID_VERTS * std::mem::size_of::<LineVertex>()) as u64, vk::BufferUsageFlags::VERTEX_BUFFER)?;
            let staging = ctx.create_buffer((LAYER_BYTES * UPLOADS_PER_FRAME) as u64, vk::BufferUsageFlags::TRANSFER_SRC)?;
            let layouts = [scene_set_layout, comp_set_layout];
            let sets = unsafe { device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(desc_pool).set_layouts(&layouts))? };
            frames.push(FrameRes {
                cmd,
                fence,
                image_available,
                globals,
                bodies,
                rings,
                particles,
                lines,
                solids,
                staging,
                scene_set: sets[0],
                comp_set: sets[1],
            });
        }

        let render_finished = (0..swapchain.images.len())
            .map(|_| unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) })
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let egui = egui_ash_renderer::Renderer::with_default_allocator(
            &ctx.instance,
            ctx.pdevice,
            device.clone(),
            egui_ash_renderer::RenderMode::RenderPass(final_pass),
            egui_ash_renderer::Options { in_flight_frames: FRAMES_IN_FLIGHT, enable_depth_test: false, enable_depth_write: false, srgb_framebuffer: false },
        )?;

        let mut r = Renderer {
            ctx,
            swapchain,
            render_finished,
            frames,
            frame_index: 0,
            cmd_pool,
            targets,
            scene_pass,
            final_pass,
            scene_set_layout,
            comp_set_layout,
            scene_layout,
            comp_layout,
            pipelines: vec![sky_pipe, planet_pipe, ring_pipe, particle_pipe, line_pipe, solid_pipe, comp_pipe],
            sky_pipe,
            planet_pipe,
            ring_pipe,
            particle_pipe,
            line_pipe,
            solid_pipe,
            comp_pipe,
            desc_pool,
            sphere_ultra,
            sphere_hi,
            sphere_lo,
            paint,
            paint_sampler,
            hdr_sampler,
            egui: Some(egui),
            needs_resize: false,
            capture: false,
            captured: None,
            capture_buf: None,
        };
        r.write_descriptors();
        Ok(r)
    }

    fn create_targets(ctx: &VkContext, sc: &Swapchain, scene_pass: vk::RenderPass, final_pass: vk::RenderPass) -> Result<Targets> {
        let (w, h) = (sc.extent.width.max(1), sc.extent.height.max(1));
        let mips = (32 - w.max(h).leading_zeros()).min(8);
        let hdr = ctx.create_image(
            w,
            h,
            mips,
            1,
            HDR_FORMAT,
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::TRANSFER_DST,
            vk::ImageAspectFlags::COLOR,
        )?;
        let hdr_attachment_view = ctx.create_view(hdr.image, HDR_FORMAT, vk::ImageViewType::TYPE_2D, vk::ImageAspectFlags::COLOR, 0, 1, 1)?;
        let depth = ctx.create_image(w, h, 1, 1, DEPTH_FORMAT, vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT, vk::ImageAspectFlags::DEPTH)?;
        let scene_fb = unsafe {
            ctx.device.create_framebuffer(
                &vk::FramebufferCreateInfo::default().render_pass(scene_pass).attachments(&[hdr_attachment_view, depth.view]).width(w).height(h).layers(1),
                None,
            )?
        };
        let final_fbs = sc
            .views
            .iter()
            .map(|v| unsafe {
                ctx.device.create_framebuffer(
                    &vk::FramebufferCreateInfo::default().render_pass(final_pass).attachments(&[*v]).width(w).height(h).layers(1),
                    None,
                )
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(Targets { hdr, hdr_attachment_view, depth, scene_fb, final_fbs })
    }

    fn destroy_targets(&self) {
        let d = &self.ctx.device;
        unsafe {
            for fb in &self.targets.final_fbs {
                d.destroy_framebuffer(*fb, None);
            }
            d.destroy_framebuffer(self.targets.scene_fb, None);
            d.destroy_image_view(self.targets.hdr_attachment_view, None);
        }
        self.ctx.destroy_image(&self.targets.hdr);
        self.ctx.destroy_image(&self.targets.depth);
    }

    fn write_descriptors(&mut self) {
        let d = &self.ctx.device;
        for f in &self.frames {
            let g = [vk::DescriptorBufferInfo { buffer: f.globals.buffer, offset: 0, range: vk::WHOLE_SIZE }];
            let b = [vk::DescriptorBufferInfo { buffer: f.bodies.buffer, offset: 0, range: vk::WHOLE_SIZE }];
            let r = [vk::DescriptorBufferInfo { buffer: f.rings.buffer, offset: 0, range: vk::WHOLE_SIZE }];
            let p = [vk::DescriptorBufferInfo { buffer: f.particles.buffer, offset: 0, range: vk::WHOLE_SIZE }];
            let img = [vk::DescriptorImageInfo { sampler: vk::Sampler::null(), image_view: self.paint.view, image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL }];
            let smp = [vk::DescriptorImageInfo { sampler: self.paint_sampler, image_view: vk::ImageView::null(), image_layout: vk::ImageLayout::UNDEFINED }];
            let hdr = [vk::DescriptorImageInfo {
                sampler: vk::Sampler::null(),
                image_view: self.targets.hdr.view,
                image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            }];
            let hsmp = [vk::DescriptorImageInfo { sampler: self.hdr_sampler, image_view: vk::ImageView::null(), image_layout: vk::ImageLayout::UNDEFINED }];
            let w = |set, binding, ty| vk::WriteDescriptorSet::default().dst_set(set).dst_binding(binding).descriptor_type(ty);
            let writes = [
                w(f.scene_set, 0, vk::DescriptorType::UNIFORM_BUFFER).buffer_info(&g),
                w(f.scene_set, 1, vk::DescriptorType::STORAGE_BUFFER).buffer_info(&b),
                w(f.scene_set, 2, vk::DescriptorType::STORAGE_BUFFER).buffer_info(&r),
                w(f.scene_set, 3, vk::DescriptorType::STORAGE_BUFFER).buffer_info(&p),
                w(f.scene_set, 4, vk::DescriptorType::SAMPLED_IMAGE).image_info(&img),
                w(f.scene_set, 5, vk::DescriptorType::SAMPLER).image_info(&smp),
                w(f.comp_set, 0, vk::DescriptorType::UNIFORM_BUFFER).buffer_info(&g),
                w(f.comp_set, 1, vk::DescriptorType::SAMPLED_IMAGE).image_info(&hdr),
                w(f.comp_set, 2, vk::DescriptorType::SAMPLER).image_info(&hsmp),
            ];
            unsafe { d.update_descriptor_sets(&writes, &[]) };
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        unsafe { self.ctx.device.device_wait_idle()? };
        self.destroy_targets();
        let old = self.swapchain.swapchain;
        let new = Swapchain::new(&self.ctx, width, height, old)?;
        self.swapchain.destroy(&self.ctx);
        self.swapchain = new;
        self.targets = Self::create_targets(&self.ctx, &self.swapchain, self.scene_pass, self.final_pass)?;
        unsafe {
            for s in &self.render_finished {
                self.ctx.device.destroy_semaphore(*s, None);
            }
        }
        self.render_finished = (0..self.swapchain.images.len())
            .map(|_| unsafe { self.ctx.device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        self.write_descriptors();
        self.needs_resize = false;
        Ok(())
    }

    pub fn extent(&self) -> vk::Extent2D {
        self.swapchain.extent
    }

    /// Render one frame. Returns Ok(false) if the swapchain must be recreated.
    pub fn render(
        &mut self,
        frame: &FrameData,
        paint: &mut PaintStore,
        egui_prims: &[egui::ClippedPrimitive],
        textures: &egui::TexturesDelta,
        pixels_per_point: f32,
    ) -> Result<bool> {
        let device = self.ctx.device.clone();
        let fi = self.frame_index;
        let f = &self.frames[fi];
        unsafe { device.wait_for_fences(&[f.fence], true, u64::MAX)? };

        // egui texture uploads (font atlas etc.) - rare, so a full idle is fine.
        // egui hands each delta over exactly once, so apply it before anything
        // that can bail out of this frame (an out-of-date swapchain mid-resize).
        if !textures.set.is_empty() {
            unsafe { device.device_wait_idle()? };
            let egui = self.egui.as_mut().unwrap();
            for (id, deltas) in &textures.set {
                for delta in deltas {
                    egui.set_texture(self.ctx.queue, self.cmd_pool, *id, delta)?;
                }
            }
        }

        let (image_index, _) = match unsafe {
            self.ctx.swapchain_fn.acquire_next_image(self.swapchain.swapchain, u64::MAX, f.image_available, vk::Fence::null())
        } {
            Ok(r) => r,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                self.free_textures(textures)?;
                return Ok(false);
            }
            Err(e) => return Err(e.into()),
        };

        unsafe { device.reset_fences(&[f.fence])? };
        let f = &self.frames[fi];

        // ---- upload frame data ----
        f.globals.write(std::slice::from_ref(&frame.globals));
        let n_bodies = f.bodies.write(&frame.bodies[..frame.bodies.len().min(MAX_BODIES)]) as u32;
        let n_rings = f.rings.write(&frame.rings[..frame.rings.len().min(MAX_RINGS)]) as u32;
        let n_particles = f.particles.write(&frame.particles[..frame.particles.len().min(MAX_PARTICLES)]) as u32;
        let n_lines = f.lines.write(&frame.lines[..frame.lines.len().min(MAX_LINE_VERTS) & !1]) as u32;
        let n_solids = (f.solids.write(&frame.solids[..frame.solids.len().min(MAX_SOLID_VERTS)]) as u32 / 3) * 3;
        let ultra = frame.ultra_count.min(n_bodies);
        let hi = frame.hi_count.min(n_bodies - ultra);

        // Dirty paint layers -> staging.
        let mut uploads = Vec::new();
        for layer in 0..MAX_LAYERS {
            if paint.dirty[layer] && uploads.len() < UPLOADS_PER_FRAME {
                let slot = uploads.len();
                f.staging.write_at(slot * LAYER_BYTES, bytemuck::cast_slice(&paint.layers[layer]));
                uploads.push((slot, layer));
                paint.dirty[layer] = false;
            }
        }

        let cmd = f.cmd;
        let extent = self.swapchain.extent;
        unsafe {
            device.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
            device.begin_command_buffer(cmd, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT))?;

            if !uploads.is_empty() {
                let range = vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: MAX_LAYERS as u32,
                };
                let to_dst = vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::SHADER_READ)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .image(self.paint.image)
                    .subresource_range(range);
                device.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::VERTEX_SHADER | vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[to_dst],
                );
                let regions: Vec<vk::BufferImageCopy> = uploads
                    .iter()
                    .map(|(slot, layer)| vk::BufferImageCopy {
                        buffer_offset: (slot * LAYER_BYTES) as u64,
                        buffer_row_length: 0,
                        buffer_image_height: 0,
                        image_subresource: vk::ImageSubresourceLayers {
                            aspect_mask: vk::ImageAspectFlags::COLOR,
                            mip_level: 0,
                            base_array_layer: *layer as u32,
                            layer_count: 1,
                        },
                        image_offset: vk::Offset3D::default(),
                        image_extent: vk::Extent3D { width: PAINT_W as u32, height: PAINT_H as u32, depth: 1 },
                    })
                    .collect();
                device.cmd_copy_buffer_to_image(cmd, f.staging.buffer, self.paint.image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &regions);
                let to_read = vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .image(self.paint.image)
                    .subresource_range(range);
                device.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::VERTEX_SHADER | vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[to_read],
                );
            }

            // ---- HDR scene pass ----
            let clears = [
                vk::ClearValue { color: vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] } },
                vk::ClearValue { depth_stencil: vk::ClearDepthStencilValue { depth: 0.0, stencil: 0 } },
            ];
            let rect = vk::Rect2D { offset: vk::Offset2D::default(), extent };
            device.cmd_begin_render_pass(
                cmd,
                &vk::RenderPassBeginInfo::default().render_pass(self.scene_pass).framebuffer(self.targets.scene_fb).render_area(rect).clear_values(&clears),
                vk::SubpassContents::INLINE,
            );
            let viewport = vk::Viewport { x: 0.0, y: 0.0, width: extent.width as f32, height: extent.height as f32, min_depth: 0.0, max_depth: 1.0 };
            device.cmd_set_viewport(cmd, 0, &[viewport]);
            device.cmd_set_scissor(cmd, 0, &[rect]);
            device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::GRAPHICS, self.scene_layout, 0, &[f.scene_set], &[]);

            if n_bodies > 0 {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.planet_pipe);
                // Instances are ordered ultra, hi, lo; each run uses a coarser sphere.
                let mut first = 0;
                for (mesh, count) in [(&self.sphere_ultra, ultra), (&self.sphere_hi, hi), (&self.sphere_lo, n_bodies - ultra - hi)] {
                    if count > 0 {
                        device.cmd_bind_vertex_buffers(cmd, 0, &[mesh.vertices.buffer], &[0]);
                        device.cmd_bind_index_buffer(cmd, mesh.indices.buffer, 0, vk::IndexType::UINT32);
                        device.cmd_draw_indexed(cmd, mesh.count, count, 0, 0, first);
                    }
                    first += count;
                }
            }
            if n_solids > 0 {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.solid_pipe);
                device.cmd_bind_vertex_buffers(cmd, 0, &[f.solids.buffer], &[0]);
                device.cmd_draw(cmd, n_solids, 1, 0, 0);
            }
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.sky_pipe);
            device.cmd_draw(cmd, 3, 1, 0, 0);
            if n_rings > 0 {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.ring_pipe);
                device.cmd_draw(cmd, 6, n_rings, 0, 0);
            }
            if n_lines > 0 {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.line_pipe);
                device.cmd_bind_vertex_buffers(cmd, 0, &[f.lines.buffer], &[0]);
                device.cmd_draw(cmd, n_lines, 1, 0, 0);
            }
            if n_particles > 0 {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.particle_pipe);
                device.cmd_draw(cmd, 6, n_particles, 0, 0);
            }
            device.cmd_end_render_pass(cmd);

            // ---- bloom mip chain via blits ----
            let hdr = &self.targets.hdr;
            let sub = |level: u32, count: u32| vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: level,
                level_count: count,
                base_array_layer: 0,
                layer_count: 1,
            };
            if hdr.mips > 1 {
                let b = vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .image(hdr.image)
                    .subresource_range(sub(1, hdr.mips - 1));
                device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[b]);
            }
            let (mut w, mut h) = (hdr.width as i32, hdr.height as i32);
            for level in 1..hdr.mips {
                let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
                let blit = vk::ImageBlit {
                    src_subresource: vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: level - 1, base_array_layer: 0, layer_count: 1 },
                    src_offsets: [vk::Offset3D { x: 0, y: 0, z: 0 }, vk::Offset3D { x: w, y: h, z: 1 }],
                    dst_subresource: vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: level, base_array_layer: 0, layer_count: 1 },
                    dst_offsets: [vk::Offset3D { x: 0, y: 0, z: 0 }, vk::Offset3D { x: nw, y: nh, z: 1 }],
                };
                device.cmd_blit_image(
                    cmd,
                    hdr.image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    hdr.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[blit],
                    vk::Filter::LINEAR,
                );
                let b = vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                    .image(hdr.image)
                    .subresource_range(sub(level, 1));
                device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[b]);
                w = nw;
                h = nh;
            }
            let b = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE | vk::AccessFlags::TRANSFER_READ)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .image(hdr.image)
                .subresource_range(sub(0, hdr.mips));
            device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::FRAGMENT_SHADER, vk::DependencyFlags::empty(), &[], &[], &[b]);

            // ---- composite + UI ----
            let clears = [vk::ClearValue { color: vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] } }];
            device.cmd_begin_render_pass(
                cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(self.final_pass)
                    .framebuffer(self.targets.final_fbs[image_index as usize])
                    .render_area(rect)
                    .clear_values(&clears),
                vk::SubpassContents::INLINE,
            );
            device.cmd_set_viewport(cmd, 0, &[viewport]);
            device.cmd_set_scissor(cmd, 0, &[rect]);
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.comp_pipe);
            device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::GRAPHICS, self.comp_layout, 0, &[f.comp_set], &[]);
            device.cmd_draw(cmd, 3, 1, 0, 0);
            self.egui.as_mut().unwrap().cmd_draw(cmd, extent, pixels_per_point, egui_prims)?;
            device.cmd_end_render_pass(cmd);
            let capturing = self.capture;
            if capturing {
                let size = (extent.width * extent.height * 4) as u64;
                if self.capture_buf.as_ref().map(|b| b.size < size).unwrap_or(true) {
                    if let Some(b) = self.capture_buf.take() {
                        self.ctx.destroy_buffer(&b);
                    }
                    self.capture_buf = Some(self.ctx.create_buffer(size, vk::BufferUsageFlags::TRANSFER_DST)?);
                }
                let image = self.swapchain.images[image_index as usize];
                let range = vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 };
                let b = vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                    .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                    .image(image)
                    .subresource_range(range);
                device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[b]);
                let region = vk::BufferImageCopy {
                    buffer_offset: 0,
                    buffer_row_length: 0,
                    buffer_image_height: 0,
                    image_subresource: vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: 0, base_array_layer: 0, layer_count: 1 },
                    image_offset: vk::Offset3D::default(),
                    image_extent: vk::Extent3D { width: extent.width, height: extent.height, depth: 1 },
                };
                device.cmd_copy_image_to_buffer(cmd, image, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, self.capture_buf.as_ref().unwrap().buffer, &[region]);
                let b = vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                    .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                    .src_access_mask(vk::AccessFlags::TRANSFER_READ)
                    .dst_access_mask(vk::AccessFlags::empty())
                    .image(image)
                    .subresource_range(range);
                device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::BOTTOM_OF_PIPE, vk::DependencyFlags::empty(), &[], &[], &[b]);
            }
            device.end_command_buffer(cmd)?;

            let wait = [f.image_available];
            let stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
            let signal = [self.render_finished[image_index as usize]];
            let cmds = [cmd];
            let submit = vk::SubmitInfo::default().wait_semaphores(&wait).wait_dst_stage_mask(&stages).command_buffers(&cmds).signal_semaphores(&signal);
            device.queue_submit(self.ctx.queue, &[submit], f.fence)?;

            let swapchains = [self.swapchain.swapchain];
            let indices = [image_index];
            let present = vk::PresentInfoKHR::default().wait_semaphores(&signal).swapchains(&swapchains).image_indices(&indices);
            let ok = match self.ctx.swapchain_fn.queue_present(self.ctx.queue, &present) {
                Ok(suboptimal) => !suboptimal,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => false,
                Err(e) => return Err(e.into()),
            };
            self.frame_index = (self.frame_index + 1) % FRAMES_IN_FLIGHT;
            if capturing {
                device.wait_for_fences(&[self.frames[fi].fence], true, u64::MAX)?;
                let buf = self.capture_buf.as_ref().unwrap();
                let n = (extent.width * extent.height * 4) as usize;
                let data = std::slice::from_raw_parts(buf.ptr, n).to_vec();
                self.captured = Some((extent.width, extent.height, data));
                self.capture = false;
            }

            self.free_textures(textures)?;
            Ok(ok)
        }
    }

    fn free_textures(&mut self, textures: &egui::TexturesDelta) -> Result<()> {
        if !textures.free.is_empty() {
            unsafe { self.ctx.device.device_wait_idle()? };
            for id in &textures.free {
                self.egui.as_mut().unwrap().free_texture(*id)?;
            }
        }
        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            let d = &self.ctx.device;
            let _ = d.device_wait_idle();
            self.egui = None;
            if let Some(b) = self.capture_buf.take() {
                self.ctx.destroy_buffer(&b);
            }
            for f in &self.frames {
                d.destroy_fence(f.fence, None);
                d.destroy_semaphore(f.image_available, None);
                for b in [&f.globals, &f.bodies, &f.rings, &f.particles, &f.lines, &f.solids, &f.staging] {
                    self.ctx.destroy_buffer(b);
                }
            }
            for s in &self.render_finished {
                d.destroy_semaphore(*s, None);
            }
            for m in [&self.sphere_ultra, &self.sphere_hi, &self.sphere_lo] {
                self.ctx.destroy_buffer(&m.vertices);
                self.ctx.destroy_buffer(&m.indices);
            }
            self.destroy_targets();
            self.ctx.destroy_image(&self.paint);
            d.destroy_sampler(self.paint_sampler, None);
            d.destroy_sampler(self.hdr_sampler, None);
            for p in &self.pipelines {
                d.destroy_pipeline(*p, None);
            }
            d.destroy_pipeline_layout(self.scene_layout, None);
            d.destroy_pipeline_layout(self.comp_layout, None);
            d.destroy_descriptor_set_layout(self.scene_set_layout, None);
            d.destroy_descriptor_set_layout(self.comp_set_layout, None);
            d.destroy_descriptor_pool(self.desc_pool, None);
            d.destroy_render_pass(self.scene_pass, None);
            d.destroy_render_pass(self.final_pass, None);
            d.destroy_command_pool(self.cmd_pool, None);
            self.swapchain.destroy(&self.ctx);
            d.destroy_device(None);
            self.ctx.surface_fn.destroy_surface(self.ctx.surface, None);
            self.ctx.instance.destroy_instance(None);
        }
    }
}
