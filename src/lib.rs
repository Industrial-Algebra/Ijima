//! Core library for Ijima

use std::marker::PhantomData;
use rayon::prelude::*;
use wgpu::util::DeviceExt;

/// Phantom type representing a unit of measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unit<T, U> {
    value: T,
    _unit: PhantomData<U>,
}

impl<T, U> Unit<T, U> {
    pub fn new(value: T) -> Self {
        Self { value, _unit: PhantomData }
    }
    pub fn value(&self) -> &T {
        &self.value
    }
}

/// Algebraic enum for a simple computation result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputeResult<T> {
    Success(T),
    Failure,
}

/// Compute the sum of squares of a slice in parallel using Rayon.
pub fn sum_of_squares<T>(data: &[T]) -> ComputeResult<T>
where
    T: Send + Sync + std::ops::Mul<Output = T> + std::ops::Add<Output = T> + Default + Copy,
{
    let result = data.par_iter().map(|&x| x * x).reduce(|| T::default(), |a, b| a + b);
    ComputeResult::Success(result)
}

/// Example GPU‑accelerated vector addition using wgpu.
/// The function returns a ComputeResult with a Vec of results.
pub async fn gpu_vector_add<T>(a: &[T], b: &[T]) -> ComputeResult<Vec<T>>
where
    T: by::fmt::Debug + bytemb::Pod + bytemb::Zeroable + Copy + 'static,
{
    // Simple length check
    if a.len() != b.len() {
        return ComputeResult::Failure;
    }
    // Initialize GPU (adapter, device, queue)
    let instance = wgpu::Instance::new(wgpu::Backends::all());
    let adapter = match instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }).await {
        Some(a) => a,
        None => return ComputeResult::Failure,
    };
    let (device, queue) = match adapter.request_device(&wgpu::DeviceDescriptor {
        label: None,
        features: wgpu::Features::empty(),
        limits: wgpu::Limits::default(),
    }, None).await {
        Ok(v) => v,
        Err(_) => return ComputeResult::Failure,
    };

    // Create buffers
    let a_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("A Buffer"),
        contents: bytemb::cast_slice(a),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let b_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("B Buffer"),
        contents: bytemb::cast_slice(b),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let size = (a.len() * std::mem::size_of::<T>()) as wgpu::BufferAddress;
    let result_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Result Buffer"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    // Simple compute shader (WGSL) that adds two vectors
    let shader_src = r#"
        [[group(0), binding(0)]] var<storage, read> a: array<f32>;
        [[group(0), binding(1)]] var<storage, read> b: array<f32>;
        [[group(0), binding(2)]] var<storage, write> out: array<f32>;
        [[stage(compute), workgroup_size(64)]]
        fn main([[builtin(global_invocation_id)]] gid: vec3<u32>) {
            let i = gid.x;
            out[i] = a[i] + b[i];
        }
    "#;
    let shader = device.create_shader_module(&wgpu::ShaderModuleDescriptor {
        label: Some("Add Shader"),
        source: wgpu::ShaderSource::Wgsl(shader_src.into()),
    });

    // Pipeline
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Add Pipeline"),
        layout: None,
        module: &shader,
        entry_point: "main",
    });

    // Bind group
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Bind Group"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: a_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: b_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: result_buf.as_entire_binding() },
        ],
    });

    // Encode commands
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("Encoder") });
    {
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("Compute Pass") });
        cpass.set_pipeline(&pipeline);
        cpass.set_bind_group(0, &bind_group, &[]);
        let workgroups = ((a.len() as u32) + 63) / 64;
        cpass.dispatch_workgroups(workgroups, 1, 1);
    }
    // Copy result to a staging buffer
    let staging_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Staging Buffer"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_buffer_to_buffer(&result_buf, 0, &staging_buf, 0, size);
    let command_buffer = encoder.finish();
    queue.submit(std::iter::once(command_buffer));

    // Await buffer mapping
    let buffer_slice = staging_buf.slice(..);
    let (sender, receiver) = futures_intrusive::channel::shared::oneshot_channel();
    buffer_slice.map_async(wgpu::MapMode::Read, move |v| sender.send(v).unwrap());
    device.poll(wgpu::Maintain::Wait);
    if let Some(Ok(())) = receiver.receive().await {
        let data = buffer_slice.get_mapped_range();
        let result: Vec<T> = bytemb::cast_slice(&data).to_vec();
        drop(data);
        staging_buf.unmap();
        ComputeResult::Success(result)
    } else {
        ComputeResult::Failure
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_sum_of_squares() {
        let data = [1i32, 2, 3, 4];
        match sum_of_squares(&data) {
            ComputeResult::Success(v) => assert_eq!(v, 30),
            _ => panic!("Computation failed"),
        }
    }

    #[test]
    fn test_unit_phantom() {
        #[derive(Debug)]
        struct Meter;
        let length = Unit::<i32, Meter>::new(5);
        assert_eq!(*length.value(), 5);
    }
}
