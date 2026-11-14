//! Vulkan bootstrap: loader discovery (MoltenVK on macOS), instance, device,
//! swapchain and small allocation helpers.

use std::ffi::{c_char, CStr, CString};
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use ash::{khr, vk, Device, Entry, Instance};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

#[allow(dead_code)]
pub struct VkContext {
    pub entry: Entry,
    pub instance: Instance,
    pub surface_fn: khr::surface::Instance,
    pub surface: vk::SurfaceKHR,
    pub pdevice: vk::PhysicalDevice,
    pub device: Device,
    pub queue: vk::Queue,
    pub queue_family: u32,
    pub swapchain_fn: khr::swapchain::Device,
    pub mem_props: vk::PhysicalDeviceMemoryProperties,
    pub device_name: String,
}

/// Find a Vulkan implementation. Order: explicit env var, MoltenVK bundled
/// inside the .app, the system Vulkan loader, then Homebrew's MoltenVK directly.
fn load_entry() -> Result<(Entry, String)> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("AUP_VULKAN_LIB") {
        candidates.push(p.into());
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("../Frameworks/libMoltenVK.dylib"));
            candidates.push(dir.join("libMoltenVK.dylib"));
        }
    }
    for p in [
        "libvulkan.1.dylib",
        "libvulkan.dylib",
        "/opt/homebrew/lib/libvulkan.1.dylib",
        "/usr/local/lib/libvulkan.1.dylib",
        "/opt/homebrew/lib/libMoltenVK.dylib",
        "/usr/local/lib/libMoltenVK.dylib",
        "libvulkan.so.1",
        "vulkan-1.dll",
    ] {
        candidates.push(p.into());
    }
    let mut errors = Vec::new();
    for c in candidates {
        let is_bare = c.components().count() == 1;
        if !is_bare && !c.exists() {
            continue;
        }
        match unsafe { Entry::load_from(&c) } {
            Ok(e) => return Ok((e, c.display().to_string())),
            Err(err) => errors.push(format!("{}: {err}", c.display())),
        }
    }
    Err(anyhow!(
        "Could not find a Vulkan implementation. On macOS install MoltenVK: `brew install molten-vk vulkan-loader`.\n{}",
        errors.join("\n")
    ))
}

impl VkContext {
    pub fn new(window: &(impl HasDisplayHandle + HasWindowHandle)) -> Result<Self> {
        let (entry, lib) = load_entry()?;
        log(&format!("Vulkan library: {lib}"));
        let display = window.display_handle()?.as_raw();
        let win = window.window_handle()?.as_raw();

        let available: Vec<CString> = unsafe { entry.enumerate_instance_extension_properties(None)? }
            .iter()
            .map(|e| unsafe { CStr::from_ptr(e.extension_name.as_ptr()) }.to_owned())
            .collect();
        let has = |name: &CStr| available.iter().any(|e| e.as_c_str() == name);

        let mut exts: Vec<*const c_char> = ash_window::enumerate_required_extensions(display)?.to_vec();
        let mut flags = vk::InstanceCreateFlags::empty();
        if has(khr::portability_enumeration::NAME) {
            exts.push(khr::portability_enumeration::NAME.as_ptr());
            flags |= vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR;
        }
        if has(khr::get_physical_device_properties2::NAME) {
            exts.push(khr::get_physical_device_properties2::NAME.as_ptr());
        }

        let app_name = c"Arthur's Universe Playground";
        let api = match unsafe { entry.try_enumerate_instance_version()? } {
            Some(v) if v >= vk::API_VERSION_1_2 => vk::API_VERSION_1_2,
            Some(v) if v >= vk::API_VERSION_1_1 => vk::API_VERSION_1_1,
            _ => vk::API_VERSION_1_0,
        };
        let app_info = vk::ApplicationInfo::default()
            .application_name(app_name)
            .application_version(1)
            .engine_name(c"ArthurEngine")
            .api_version(api);
        let create_info = vk::InstanceCreateInfo::default().application_info(&app_info).enabled_extension_names(&exts).flags(flags);
        let instance = unsafe { entry.create_instance(&create_info, None) }.context("vkCreateInstance")?;

        let surface = unsafe { ash_window::create_surface(&entry, &instance, display, win, None)? };
        let surface_fn = khr::surface::Instance::new(&entry, &instance);

        // Pick a GPU with a graphics queue that can present to our surface.
        let pdevices = unsafe { instance.enumerate_physical_devices()? };
        let mut chosen = None;
        for pd in pdevices {
            let props = unsafe { instance.get_physical_device_properties(pd) };
            let qfs = unsafe { instance.get_physical_device_queue_family_properties(pd) };
            for (i, q) in qfs.iter().enumerate() {
                let present = unsafe { surface_fn.get_physical_device_surface_support(pd, i as u32, surface)? };
                if q.queue_flags.contains(vk::QueueFlags::GRAPHICS) && present {
                    let score = match props.device_type {
                        vk::PhysicalDeviceType::DISCRETE_GPU => 3,
                        vk::PhysicalDeviceType::INTEGRATED_GPU => 2,
                        _ => 1,
                    };
                    if chosen.map(|(_, _, s)| score > s).unwrap_or(true) {
                        chosen = Some((pd, i as u32, score));
                    }
                    break;
                }
            }
        }
        let (pdevice, queue_family, _) = chosen.ok_or_else(|| anyhow!("No Vulkan GPU with graphics+present support found"))?;
        let props = unsafe { instance.get_physical_device_properties(pdevice) };
        let device_name = unsafe { CStr::from_ptr(props.device_name.as_ptr()) }.to_string_lossy().to_string();
        log(&format!("GPU: {device_name}"));

        let dev_exts_avail: Vec<CString> = unsafe { instance.enumerate_device_extension_properties(pdevice)? }
            .iter()
            .map(|e| unsafe { CStr::from_ptr(e.extension_name.as_ptr()) }.to_owned())
            .collect();
        let mut dev_exts = vec![khr::swapchain::NAME.as_ptr()];
        if dev_exts_avail.iter().any(|e| e.as_c_str() == khr::portability_subset::NAME) {
            dev_exts.push(khr::portability_subset::NAME.as_ptr());
        }
        let priorities = [1.0];
        let queue_info = [vk::DeviceQueueCreateInfo::default().queue_family_index(queue_family).queue_priorities(&priorities)];
        let features = vk::PhysicalDeviceFeatures::default();
        let device_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_info)
            .enabled_extension_names(&dev_exts)
            .enabled_features(&features);
        let device = unsafe { instance.create_device(pdevice, &device_info, None) }.context("vkCreateDevice")?;
        let queue = unsafe { device.get_device_queue(queue_family, 0) };
        let swapchain_fn = khr::swapchain::Device::new(&instance, &device);
        let mem_props = unsafe { instance.get_physical_device_memory_properties(pdevice) };

        Ok(VkContext { entry, instance, surface_fn, surface, pdevice, device, queue, queue_family, swapchain_fn, mem_props, device_name })
    }

    pub fn find_memory_type(&self, bits: u32, props: vk::MemoryPropertyFlags) -> Result<u32> {
        for i in 0..self.mem_props.memory_type_count {
            if bits & (1 << i) != 0 && self.mem_props.memory_types[i as usize].property_flags.contains(props) {
                return Ok(i);
            }
        }
        Err(anyhow!("no suitable memory type"))
    }

    pub fn create_buffer(&self, size: u64, usage: vk::BufferUsageFlags) -> Result<Buffer> {
        let size = size.max(16);
        let info = vk::BufferCreateInfo::default().size(size).usage(usage).sharing_mode(vk::SharingMode::EXCLUSIVE);
        let buffer = unsafe { self.device.create_buffer(&info, None)? };
        let req = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let mt = self.find_memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT)?;
        let alloc = vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(mt);
        let memory = unsafe { self.device.allocate_memory(&alloc, None)? };
        unsafe { self.device.bind_buffer_memory(buffer, memory, 0)? };
        let ptr = unsafe { self.device.map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())? } as *mut u8;
        Ok(Buffer { buffer, memory, size, ptr })
    }

    pub fn destroy_buffer(&self, b: &Buffer) {
        unsafe {
            self.device.unmap_memory(b.memory);
            self.device.destroy_buffer(b.buffer, None);
            self.device.free_memory(b.memory, None);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_image(
        &self,
        width: u32,
        height: u32,
        mips: u32,
        layers: u32,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        aspect: vk::ImageAspectFlags,
    ) -> Result<Image> {
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D { width, height, depth: 1 })
            .mip_levels(mips)
            .array_layers(layers)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let image = unsafe { self.device.create_image(&info, None)? };
        let req = unsafe { self.device.get_image_memory_requirements(image) };
        let mt = self.find_memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
        let alloc = vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(mt);
        let memory = unsafe { self.device.allocate_memory(&alloc, None)? };
        unsafe { self.device.bind_image_memory(image, memory, 0)? };
        let view_type = if layers > 1 { vk::ImageViewType::TYPE_2D_ARRAY } else { vk::ImageViewType::TYPE_2D };
        let view = self.create_view(image, format, view_type, aspect, 0, mips, layers)?;
        Ok(Image { image, memory, view, width, height, mips, layers, format })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_view(
        &self,
        image: vk::Image,
        format: vk::Format,
        view_type: vk::ImageViewType,
        aspect: vk::ImageAspectFlags,
        base_mip: u32,
        mips: u32,
        layers: u32,
    ) -> Result<vk::ImageView> {
        let info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(view_type)
            .format(format)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: aspect,
                base_mip_level: base_mip,
                level_count: mips,
                base_array_layer: 0,
                layer_count: layers,
            });
        Ok(unsafe { self.device.create_image_view(&info, None)? })
    }

    pub fn destroy_image(&self, img: &Image) {
        unsafe {
            self.device.destroy_image_view(img.view, None);
            self.device.destroy_image(img.image, None);
            self.device.free_memory(img.memory, None);
        }
    }

    /// Record and synchronously execute a one-off command buffer.
    pub fn one_time(&self, pool: vk::CommandPool, f: impl FnOnce(vk::CommandBuffer)) -> Result<()> {
        unsafe {
            let info = vk::CommandBufferAllocateInfo::default().command_pool(pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1);
            let cmd = self.device.allocate_command_buffers(&info)?[0];
            self.device.begin_command_buffer(cmd, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT))?;
            f(cmd);
            self.device.end_command_buffer(cmd)?;
            let cmds = [cmd];
            let submit = [vk::SubmitInfo::default().command_buffers(&cmds)];
            self.device.queue_submit(self.queue, &submit, vk::Fence::null())?;
            self.device.queue_wait_idle(self.queue)?;
            self.device.free_command_buffers(pool, &cmds);
        }
        Ok(())
    }
}

pub struct Buffer {
    pub buffer: vk::Buffer,
    pub memory: vk::DeviceMemory,
    pub size: u64,
    pub ptr: *mut u8,
}

impl Buffer {
    /// Copy POD data into the persistently mapped buffer; returns number of elements written.
    pub fn write<T: bytemuck::Pod>(&self, data: &[T]) -> usize {
        let bytes: &[u8] = bytemuck::cast_slice(data);
        let n = bytes.len().min(self.size as usize);
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.ptr, n) };
        n / std::mem::size_of::<T>().max(1)
    }
    pub fn write_at(&self, offset: usize, bytes: &[u8]) {
        let n = bytes.len().min((self.size as usize).saturating_sub(offset));
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.ptr.add(offset), n) };
    }
}

#[allow(dead_code)]
pub struct Image {
    pub image: vk::Image,
    pub memory: vk::DeviceMemory,
    pub view: vk::ImageView,
    pub width: u32,
    pub height: u32,
    pub mips: u32,
    pub layers: u32,
    pub format: vk::Format,
}

pub struct Swapchain {
    pub swapchain: vk::SwapchainKHR,
    pub images: Vec<vk::Image>,
    pub views: Vec<vk::ImageView>,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
}

impl Swapchain {
    pub fn new(ctx: &VkContext, width: u32, height: u32, old: vk::SwapchainKHR) -> Result<Swapchain> {
        let caps = unsafe { ctx.surface_fn.get_physical_device_surface_capabilities(ctx.pdevice, ctx.surface)? };
        let formats = unsafe { ctx.surface_fn.get_physical_device_surface_formats(ctx.pdevice, ctx.surface)? };
        let format = formats
            .iter()
            .find(|f| f.format == vk::Format::B8G8R8A8_UNORM && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR)
            .or_else(|| formats.iter().find(|f| f.format == vk::Format::R8G8B8A8_UNORM))
            .copied()
            .unwrap_or(formats[0]);
        let extent = if caps.current_extent.width != u32::MAX {
            caps.current_extent
        } else {
            vk::Extent2D {
                width: width.clamp(caps.min_image_extent.width, caps.max_image_extent.width),
                height: height.clamp(caps.min_image_extent.height, caps.max_image_extent.height),
            }
        };
        let mut count = caps.min_image_count + 1;
        if caps.max_image_count > 0 {
            count = count.min(caps.max_image_count);
        }
        let alpha = if caps.supported_composite_alpha.contains(vk::CompositeAlphaFlagsKHR::OPAQUE) {
            vk::CompositeAlphaFlagsKHR::OPAQUE
        } else {
            vk::CompositeAlphaFlagsKHR::INHERIT
        };
        let info = vk::SwapchainCreateInfoKHR::default()
            .surface(ctx.surface)
            .min_image_count(count)
            .image_format(format.format)
            .image_color_space(format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | (caps.supported_usage_flags & vk::ImageUsageFlags::TRANSFER_SRC))
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(caps.current_transform)
            .composite_alpha(alpha)
            .present_mode(vk::PresentModeKHR::FIFO)
            .clipped(true)
            .old_swapchain(old);
        let swapchain = unsafe { ctx.swapchain_fn.create_swapchain(&info, None)? };
        let images = unsafe { ctx.swapchain_fn.get_swapchain_images(swapchain)? };
        let views = images
            .iter()
            .map(|&img| ctx.create_view(img, format.format, vk::ImageViewType::TYPE_2D, vk::ImageAspectFlags::COLOR, 0, 1, 1))
            .collect::<Result<Vec<_>>>()?;
        Ok(Swapchain { swapchain, images, views, format: format.format, extent })
    }

    pub fn destroy(&self, ctx: &VkContext) {
        unsafe {
            for v in &self.views {
                ctx.device.destroy_image_view(*v, None);
            }
            ctx.swapchain_fn.destroy_swapchain(self.swapchain, None);
        }
    }
}

pub fn log(msg: &str) {
    eprintln!("[arthur] {msg}");
}
