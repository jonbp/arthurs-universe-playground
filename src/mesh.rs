use std::collections::HashMap;

/// Unit icosphere: (positions, triangle indices).
pub fn icosphere(subdivisions: u32) -> (Vec<[f32; 3]>, Vec<u32>) {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let mut verts: Vec<[f32; 3]> = vec![
        [-1.0, t, 0.0],
        [1.0, t, 0.0],
        [-1.0, -t, 0.0],
        [1.0, -t, 0.0],
        [0.0, -1.0, t],
        [0.0, 1.0, t],
        [0.0, -1.0, -t],
        [0.0, 1.0, -t],
        [t, 0.0, -1.0],
        [t, 0.0, 1.0],
        [-t, 0.0, -1.0],
        [-t, 0.0, 1.0],
    ];
    for v in &mut verts {
        *v = normalize(*v);
    }
    let mut faces: Vec<[u32; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    for _ in 0..subdivisions {
        let mut cache: HashMap<(u32, u32), u32> = HashMap::new();
        let mut mid = |a: u32, b: u32, verts: &mut Vec<[f32; 3]>| -> u32 {
            let key = if a < b { (a, b) } else { (b, a) };
            if let Some(&i) = cache.get(&key) {
                return i;
            }
            let (va, vb) = (verts[a as usize], verts[b as usize]);
            let m = normalize([(va[0] + vb[0]) * 0.5, (va[1] + vb[1]) * 0.5, (va[2] + vb[2]) * 0.5]);
            verts.push(m);
            let i = verts.len() as u32 - 1;
            cache.insert(key, i);
            i
        };
        let mut next = Vec::with_capacity(faces.len() * 4);
        for f in &faces {
            let a = mid(f[0], f[1], &mut verts);
            let b = mid(f[1], f[2], &mut verts);
            let c = mid(f[2], f[0], &mut verts);
            next.push([f[0], a, c]);
            next.push([f[1], b, a]);
            next.push([f[2], c, b]);
            next.push([a, b, c]);
        }
        faces = next;
    }
    let indices = faces.iter().flat_map(|f| f.iter().copied()).collect();
    (verts, indices)
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / l, v[1] / l, v[2] / l]
}
