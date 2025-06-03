use wgpu::{util::DeviceExt, *};
use bytemuck::{Pod, Zeroable};
use futures_intrusive::channel::shared;
use anyhow::{Result, Context, anyhow};
use std::time::Duration;
use log::{warn, error};
use futures::FutureExt;

use crate::video::VideoFrame;

// GPU context for processing frames
pub struct GpuProcessor {
    device: Device,
    queue: Queue,
    _staging_belt: util::StagingBelt,
    texture_format: TextureFormat,
    resize_pipeline: ComputePipeline,
    bind_group_layout: BindGroupLayout,
    // Cached resources
    input_texture: Option<(u32, u32, Texture)>,
    output_texture: Option<(u32, u32, Texture)>,
    output_buffer: Option<(u32, u32, Buffer)>,
    uniform_buffer: Option<Buffer>,
    bind_group: Option<(u32, u32, u32, u32, BindGroup)>,
}

// Define a structure for our resize parameters
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
struct ResizeParams {
    input_width: f32,
    input_height: f32,
    output_width: f32,
    output_height: f32,
}

impl GpuProcessor {
    pub async fn new() -> Result<Self, anyhow::Error> {
        // Set up GPU instance with better error handling
        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::all(),
            flags: InstanceFlags::default(),
            backend_options: Default::default(),
        });

        // Find a suitable adapter (prefer high performance GPU)
        let adapter_result = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await;
            
        // Handle fallbacks for adapter acquisition
        let adapter = match adapter_result {
            Ok(adapter) => adapter,
            Err(_) => {
                // Fallback to any adapter if high performance is not available
                warn!("High performance GPU not available, falling back to any adapter");
                match instance
                    .request_adapter(&RequestAdapterOptions {
                        power_preference: PowerPreference::LowPower,
                        compatible_surface: None,
                        force_fallback_adapter: true,
                    })
                    .await {
                    Ok(adapter) => adapter,
                    Err(e) => return Err(anyhow!("No GPU adapters found: {:?}", e)),
                }
            }
        };

        // Create device and queue with error handling
        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    label: Some("Video processor"),
                    required_features: Features::empty(),
                    required_limits: Limits::default(),
                    memory_hints: wgpu::MemoryHints::default(),
                    trace: wgpu::Trace::default(),
                }
            )
            .await
            .context("Failed to create GPU device")?;

        // Create staging belt for texture transfers
        let staging_belt = util::StagingBelt::new(1024);

        // Texture format for RGBA
        let texture_format = TextureFormat::Rgba8Unorm;

        // Load compute shader for resizing
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Resize shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/resize.wgsl").into()),
        });

        // Create bind group layout
        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Texture bind group layout"),
            entries: &[
                // Input texture
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Output texture
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::StorageTexture {
                        access: StorageTextureAccess::WriteOnly,
                        format: texture_format,
                        view_dimension: TextureViewDimension::D2,
                    },
                    count: None,
                },
                // Uniform buffer with resize parameters
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        // Create compute pipeline
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Resize pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let resize_pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("Resize pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: PipelineCompilationOptions::default(),
            cache: None,
        });

        Ok(Self {
            device,
            queue,
            _staging_belt: staging_belt,
            texture_format,
            resize_pipeline,
            bind_group_layout,
            input_texture: None,
            output_texture: None,
            output_buffer: None,
            uniform_buffer: None,
            bind_group: None,
        })
    }

    // Process a frame with GPU acceleration
    pub fn process_frame(
        &mut self,
        frame: &VideoFrame,
        target_width: u32,
        target_height: u32,
    ) -> Vec<u8> {
        let input_width = frame.width as u32;
        let input_height = frame.height as u32;

        // Get or create input texture with correct dimensions
        let input_texture = if let Some((w, h, ref texture)) = self.input_texture {
            if w == input_width && h == input_height {
                texture
            } else {
                let texture_size = wgpu::Extent3d {
                    width: input_width,
                    height: input_height,
                    depth_or_array_layers: 1,
                };
                
                let new_texture = self.device.create_texture(&TextureDescriptor {
                    label: Some("Input texture"),
                    size: texture_size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: self.texture_format,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                
                self.input_texture = Some((input_width, input_height, new_texture));
                &self.input_texture.as_ref().unwrap().2
            }
        } else {
            let texture_size = wgpu::Extent3d {
                width: input_width,
                height: input_height,
                depth_or_array_layers: 1,
            };
            
            let new_texture = self.device.create_texture(&TextureDescriptor {
                label: Some("Input texture"),
                size: texture_size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: self.texture_format,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            });
            
            self.input_texture = Some((input_width, input_height, new_texture));
            &self.input_texture.as_ref().unwrap().2
        };

        // Upload frame data to texture - this needs to happen every frame
        let texture_size = wgpu::Extent3d {
            width: input_width,
            height: input_height,
            depth_or_array_layers: 1,
        };
        
        self.queue.write_texture(
            TexelCopyTextureInfo {
                texture: input_texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            frame.as_rgba_bytes(),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * input_width),
                rows_per_image: Some(input_height),
            },
            texture_size,
        );

        // Get or create output texture with correct dimensions
        let output_texture = if let Some((w, h, ref texture)) = self.output_texture {
            if w == target_width && h == target_height {
                texture
            } else {
                let output_size = wgpu::Extent3d {
                    width: target_width,
                    height: target_height,
                    depth_or_array_layers: 1,
                };
                
                let new_texture = self.device.create_texture(&TextureDescriptor {
                    label: Some("Output texture"),
                    size: output_size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: self.texture_format,
                    usage: TextureUsages::STORAGE_BINDING | TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                
                self.output_texture = Some((target_width, target_height, new_texture));
                &self.output_texture.as_ref().unwrap().2
            }
        } else {
            let output_size = wgpu::Extent3d {
                width: target_width,
                height: target_height,
                depth_or_array_layers: 1,
            };
            
            let new_texture = self.device.create_texture(&TextureDescriptor {
                label: Some("Output texture"),
                size: output_size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: self.texture_format,
                usage: TextureUsages::STORAGE_BINDING | TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            
            self.output_texture = Some((target_width, target_height, new_texture));
            &self.output_texture.as_ref().unwrap().2
        };

        // Create output buffer for reading back the result with proper alignment
        // Ensure we allocate enough space for padded rows
        // WGPU requires rows to be aligned to 256 bytes (COPY_BYTES_PER_ROW_ALIGNMENT)
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u32;
        let bytes_per_row = 4 * target_width;
        // Calculate padding needed to ensure 256-byte alignment
        let padding = (align - (bytes_per_row % align)) % align;
        let padded_bytes_per_row = bytes_per_row + padding;
        // Make sure buffer size accounts for padding on every row
        let output_buffer_size = (padded_bytes_per_row * target_height) as u64;
        
        let output_buffer = if let Some((w, h, ref buffer)) = self.output_buffer {
            if w == target_width && h == target_height {
                buffer
            } else {
                let new_buffer = self.device.create_buffer(&BufferDescriptor {
                    label: Some("Output buffer"),
                    size: output_buffer_size,
                    usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                
                self.output_buffer = Some((target_width, target_height, new_buffer));
                &self.output_buffer.as_ref().unwrap().2
            }
        } else {
            let new_buffer = self.device.create_buffer(&BufferDescriptor {
                label: Some("Output buffer"),
                size: output_buffer_size,
                usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            
            self.output_buffer = Some((target_width, target_height, new_buffer));
            &self.output_buffer.as_ref().unwrap().2
        };

        // Create or reuse uniform buffer with resize parameters
        let resize_params = ResizeParams {
            input_width: input_width as f32,
            input_height: input_height as f32,
            output_width: target_width as f32,
            output_height: target_height as f32,
        };

        let uniform_buffer = if let Some(ref buffer) = self.uniform_buffer {
            // Update existing buffer
            self.queue.write_buffer(buffer, 0, bytemuck::cast_slice(&[resize_params]));
            buffer
        } else {
            // Create new buffer
            let new_buffer = self.device.create_buffer_init(&util::BufferInitDescriptor {
                label: Some("Resize params buffer"),
                contents: bytemuck::cast_slice(&[resize_params]),
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            });
            
            self.uniform_buffer = Some(new_buffer);
            self.uniform_buffer.as_ref().unwrap()
        };

        // Create or reuse bind group 
        let bind_group = if let Some((iw, ih, ow, oh, ref group)) = self.bind_group {
            if iw == input_width && ih == input_height && ow == target_width && oh == target_height {
                group
            } else {
                let new_group = self.device.create_bind_group(&BindGroupDescriptor {
                    label: Some("Resize bind group"),
                    layout: &self.bind_group_layout,
                    entries: &[
                        BindGroupEntry {
                            binding: 0,
                            resource: BindingResource::TextureView(
                                &input_texture.create_view(&TextureViewDescriptor::default()),
                            ),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: BindingResource::TextureView(
                                &output_texture.create_view(&TextureViewDescriptor::default()),
                            ),
                        },
                        BindGroupEntry {
                            binding: 2,
                            resource: uniform_buffer.as_entire_binding(),
                        },
                    ],
                });
                
                self.bind_group = Some((input_width, input_height, target_width, target_height, new_group));
                &self.bind_group.as_ref().unwrap().4
            }
        } else {
            let new_group = self.device.create_bind_group(&BindGroupDescriptor {
                label: Some("Resize bind group"),
                layout: &self.bind_group_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: BindingResource::TextureView(
                            &input_texture.create_view(&TextureViewDescriptor::default()),
                        ),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::TextureView(
                            &output_texture.create_view(&TextureViewDescriptor::default()),
                        ),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: uniform_buffer.as_entire_binding(),
                    },
                ],
            });
            
            self.bind_group = Some((input_width, input_height, target_width, target_height, new_group));
            &self.bind_group.as_ref().unwrap().4
        };

        // Encode compute pass - optimized for performance
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Resize encoder"),
            });

        {
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("Resize pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&self.resize_pipeline);
            compute_pass.set_bind_group(0, bind_group, &[]);

            // Use larger workgroups for better performance
            let workgroup_size = 16; // Increased from 8 to 16 for better GPU utilization
            let workgroup_count_x = (target_width + workgroup_size - 1) / workgroup_size;
            let workgroup_count_y = (target_height + workgroup_size - 1) / workgroup_size;

            compute_pass.dispatch_workgroups(workgroup_count_x, workgroup_count_y, 1);
        }

        // Copy the output texture to the buffer
        let output_size = wgpu::Extent3d {
            width: target_width,
            height: target_height,
            depth_or_array_layers: 1,
        };
        
        // Copy the output texture to the buffer with proper alignment
        // WGPU requires strict 256-byte alignment (COPY_BYTES_PER_ROW_ALIGNMENT)
        // This is critical for avoiding crashes in terminals like Kitty
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u32;
        let bytes_per_row = 4 * target_width;
        let padding = (align - (bytes_per_row % align)) % align;
        let padded_bytes_per_row = bytes_per_row + padding;
        
        // Double-check alignment to prevent crashes
        debug_assert!(padded_bytes_per_row % align == 0, 
            "Byte row not properly aligned: {padded_bytes_per_row} not divisible by {align}");
        
        encoder.copy_texture_to_buffer(
            TexelCopyTextureInfo {
                texture: output_texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            TexelCopyBufferInfo {
                buffer: output_buffer,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(target_height),
                },
            },
            output_size,
        );

        // Submit the commands
        self.queue.submit(std::iter::once(encoder.finish()));

        // Read back the output buffer
        let buffer_slice = output_buffer.slice(..);
        let (sender, receiver) = shared::oneshot_channel();
        buffer_slice.map_async(MapMode::Read, move |result| {
            // Use ok() to avoid unwrap panic if the channel is already closed
            let _ = sender.send(result).ok();
        });

        // Poll until ready, with a timeout to avoid hanging
        // Use a timeout to avoid hanging
        let poll_result = pollster::block_on(async {
            let timeout = Duration::from_millis(1000);
            let timeout_future = async_std::task::sleep(timeout);
            
            futures::select! {
                _ = timeout_future.fuse() => Err(anyhow!("GPU operation timed out")),
                result = futures::future::poll_fn(|cx| {
                    match self.device.poll(wgpu::MaintainBase::Poll) {
                        Ok(_) => std::task::Poll::Ready(Ok(())),
                        Err(_e) => {
                            // Yield to allow other tasks to progress
                            cx.waker().wake_by_ref();
                            std::task::Poll::Pending
                        }
                    }
                }).fuse() => result
            }
        });
        
        if poll_result.is_err() {
            error!("GPU operation timed out or failed");
            return vec![0; (4 * target_width * target_height) as usize];
        }

        // Get the buffer contents with safer error handling
        if let Some(Ok(())) = pollster::block_on(receiver.receive()) {
            let data = buffer_slice.get_mapped_range();
            
            // We need to account for the padding when copying back the data
            let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u32;
            let bytes_per_row = 4 * target_width;
            let padding = (align - (bytes_per_row % align)) % align;
            let padded_bytes_per_row = bytes_per_row + padding;
            
            // If there's no padding, we can just return the data directly
            if padding == 0 {
                let result = data.to_vec();
                drop(data);
                output_buffer.unmap();
                result
            } else {
                // Otherwise, we need to copy each row without the padding
                let mut result = Vec::with_capacity((4 * target_width * target_height) as usize);
                let padded_data = data.as_ref();
                
                // Safety check to ensure we don't go out of bounds
                let expected_size = (padded_bytes_per_row * target_height) as usize;
                if padded_data.len() < expected_size {
                    error!("GPU buffer is smaller than expected: {} < {}", 
                               padded_data.len(), expected_size);
                    
                    drop(data);
                    output_buffer.unmap();
                    return vec![0; (4 * target_width * target_height) as usize];
                }
                
                for row in 0..target_height {
                    let row_start = (row * padded_bytes_per_row) as usize;
                    let row_end = row_start + (bytes_per_row as usize);
                    
                    // Safety check to ensure we don't go out of bounds
                    if row_end <= padded_data.len() {
                        result.extend_from_slice(&padded_data[row_start..row_end]);
                    } else {
                        error!("Row end exceeds buffer size: {} > {}", 
                                   row_end, padded_data.len());
                        // Fill with black for any missing data
                        result.extend(vec![0; bytes_per_row as usize]);
                    }
                }
                
                drop(data);
                output_buffer.unmap();
                result
            }
        } else {
            // If there's an error, return a black frame buffer instead of panicking
            error!("Failed to read back from GPU - returning black frame");
            // We don't need to explicitly check for mapping, just unmap the buffer
            output_buffer.unmap();
            vec![0; (4 * target_width * target_height) as usize]
        }
    }
}


