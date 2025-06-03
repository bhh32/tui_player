use std::collections::{VecDeque, HashMap};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use log::{debug, trace, warn};
use super::VideoFrame;
use anyhow::Result;

/// FrameBuffer for smoother video playback
/// 
/// This buffer manages a collection of decoded video frames,
/// supporting prefetching and smooth frame delivery for better playback
pub struct FrameBuffer {
    // Buffered frames in playback order
    frames: VecDeque<VideoFrame>,
    // Fast lookup by approximate timestamp
    timestamp_map: HashMap<i64, usize>,
    // Maximum number of frames to buffer
    capacity: usize,
    // Prefetch flag - if true, background thread is active
    prefetching: bool,
    // Position where we're currently playing
    current_position: f64,
    // Thread handle for prefetching
    prefetch_thread: Option<JoinHandle<()>>,
    // Flag to signal prefetch thread to stop
    stop_prefetch: Arc<Mutex<bool>>,
}

impl FrameBuffer {
    /// Create a new frame buffer with the specified capacity
    pub fn new(capacity: usize) -> Self {
        FrameBuffer {
            frames: VecDeque::with_capacity(capacity),
            timestamp_map: HashMap::new(),
            capacity,
            prefetching: false,
            current_position: 0.0,
            prefetch_thread: None,
            stop_prefetch: Arc::new(Mutex::new(false)),
        }
    }
    
    /// Add a frame to the buffer, returns true if added, false if buffer is full
    pub fn add_frame(&mut self, frame: VideoFrame) -> bool {
        if self.frames.len() >= self.capacity {
            // Buffer is full, try to remove old frames first
            self.cleanup_old_frames();
            
            // If still full after cleanup, reject the frame
            if self.frames.len() >= self.capacity {
                return false;
            }
        }
        
        // Store timestamp for quick lookup (convert to milliseconds for integer key)
        let timestamp_key = (frame.timestamp * 1000.0) as i64;
        let index = self.frames.len();
        self.timestamp_map.insert(timestamp_key, index);
        
        // Add the frame to the buffer
        self.frames.push_back(frame);
        true
    }
    
    /// Get the frame closest to the requested timestamp
    pub fn get_frame_at(&self, timestamp: f64) -> Option<VideoFrame> {
        if self.frames.is_empty() {
            return None;
        }
        
        // Convert to milliseconds for lookup
        let timestamp_key = (timestamp * 1000.0) as i64;
        
        // Try exact match first
        if let Some(&index) = self.timestamp_map.get(&timestamp_key) {
            if index < self.frames.len() {
                return Some(self.frames[index].clone());
            }
        }
        
        // No exact match, find closest frame
        let mut closest_frame = None;
        let mut min_diff = f64::MAX;
        
        for frame in &self.frames {
            let diff = (frame.timestamp - timestamp).abs();
            if diff < min_diff {
                min_diff = diff;
                closest_frame = Some(frame.clone());
            }
        }
        
        closest_frame
    }
    
    /// Get the next frame in sequence based on current position
    pub fn next_frame(&mut self) -> Option<VideoFrame> {
        if self.frames.is_empty() {
            return None;
        }
        
        // Find the next frame after current position
        for (_i, frame) in self.frames.iter().enumerate() {
            if frame.timestamp > self.current_position {
                self.current_position = frame.timestamp;
                return Some(frame.clone());
            }
        }
        
        // If we're at the end, return the last frame
        let last_frame = self.frames.back()?;
        self.current_position = last_frame.timestamp;
        Some(last_frame.clone())
    }
    
    /// Get current frame (for paused state)
    pub fn current_frame(&self) -> Option<VideoFrame> {
        if self.frames.is_empty() {
            return None;
        }
        
        self.get_frame_at(self.current_position)
    }
    
    /// Seek to a specific timestamp
    pub fn seek(&mut self, timestamp: f64) {
        self.current_position = timestamp;
        
        // After seeking, clean up frames that are no longer needed
        self.cleanup_old_frames();
    }
    
    /// Get buffer status
    pub fn status(&self) -> (usize, usize, f64) {
        (self.frames.len(), self.capacity, self.current_position)
    }
    
    /// Clear all frames from the buffer
    pub fn clear(&mut self) {
        self.frames.clear();
        self.timestamp_map.clear();
        self.current_position = 0.0;
    }
    
    /// Clean up frames that are before the current position minus a small buffer
    fn cleanup_old_frames(&mut self) {
        // Keep a small buffer of past frames for rewind/seek
        let buffer_time = 2.0; // seconds
        let cutoff = self.current_position - buffer_time;
        
        // Don't clean up if we're at the beginning
        if cutoff <= 0.0 {
            return;
        }
        
        // Remove frames older than the cutoff
        let mut removed_count = 0;
        while let Some(frame) = self.frames.front() {
            if frame.timestamp >= cutoff {
                break;
            }
            
            self.frames.pop_front();
            removed_count += 1;
        }
        
        if removed_count > 0 {
            debug!("Cleaned up {} old frames from buffer", removed_count);
            
            // Rebuild timestamp map after removing frames
            self.rebuild_timestamp_map();
        }
    }
    
    /// Rebuild the timestamp map after changes to the frame collection
    fn rebuild_timestamp_map(&mut self) {
        self.timestamp_map.clear();
        for (i, frame) in self.frames.iter().enumerate() {
            let timestamp_key = (frame.timestamp * 1000.0) as i64;
            self.timestamp_map.insert(timestamp_key, i);
        }
    }
    
    /// Start prefetching frames using a decoder callback
    pub fn start_prefetching<F>(&mut self, mut decoder_callback: F) 
    where 
        F: FnMut() -> Result<Option<VideoFrame>> + Send + 'static 
    {
        if self.prefetching {
            debug!("Prefetching already in progress");
            return;
        }
        
        // Reset stop flag
        let stop_flag = self.stop_prefetch.clone();
        *stop_flag.lock().unwrap() = false;
        
        // Create a thread to prefetch frames
        let buffer_capacity = self.capacity;
        let thread_handle = thread::spawn(move || {
            debug!("Frame prefetching thread started");
            
            let mut frames_decoded = 0;
            let max_decode_errors = 5;
            let mut consecutive_errors = 0;
            
            while !*stop_flag.lock().unwrap() {
                // Check if we should continue prefetching
                if frames_decoded >= buffer_capacity {
                    trace!("Prefetch buffer full ({} frames), sleeping", frames_decoded);
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
                
                // Try to decode a frame
                match decoder_callback() {
                    Ok(Some(frame)) => {
                        frames_decoded += 1;
                        consecutive_errors = 0;
                        trace!("Prefetched frame at {}s", frame.timestamp);
                    }
                    Ok(None) => {
                        // End of stream reached
                        debug!("Prefetching complete, reached end of stream");
                        break;
                    }
                    Err(e) => {
                        consecutive_errors += 1;
                        warn!("Error during frame prefetching: {}", e);
                        
                        if consecutive_errors >= max_decode_errors {
                            warn!("Too many consecutive decode errors, stopping prefetch");
                            break;
                        }
                        
                        // Sleep before retrying
                        thread::sleep(Duration::from_millis(50));
                    }
                }
            }
            
            debug!("Frame prefetching thread finished after {} frames", frames_decoded);
        });
        
        self.prefetch_thread = Some(thread_handle);
        self.prefetching = true;
        debug!("Started frame prefetching");
    }
    
    /// Stop prefetching frames
    pub fn stop_prefetching(&mut self) {
        if !self.prefetching {
            return;
        }
        
        // Signal the thread to stop
        *self.stop_prefetch.lock().unwrap() = true;
        
        // Wait for the thread to finish
        if let Some(thread) = self.prefetch_thread.take() {
            let _ = thread.join();
        }
        
        self.prefetching = false;
        debug!("Stopped frame prefetching");
    }
}

impl Drop for FrameBuffer {
    fn drop(&mut self) {
        // Make sure to stop the prefetch thread when the buffer is dropped
        self.stop_prefetching();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbaImage};
    
    // Helper to create a test frame
    fn create_test_frame(timestamp: f64) -> VideoFrame {
        let image = DynamicImage::ImageRgba8(RgbaImage::new(10, 10));
        VideoFrame::new(image, timestamp, 1.0/30.0)
    }
    
    #[test]
    fn test_frame_buffer_add_get() {
        let mut buffer = FrameBuffer::new(5);
        
        // Add some frames
        for i in 0..5 {
            let frame = create_test_frame(i as f64);
            assert!(buffer.add_frame(frame));
        }
        
        // Buffer should be full now
        let extra_frame = create_test_frame(5.0);
        assert!(!buffer.add_frame(extra_frame));
        
        // Test retrieving frames
        let frame = buffer.get_frame_at(2.0).unwrap();
        assert_eq!(frame.timestamp, 2.0);
        
        // Test approximate match
        let frame = buffer.get_frame_at(2.1).unwrap();
        assert_eq!(frame.timestamp, 2.0);
    }
    
    #[test]
    fn test_frame_buffer_next_frame() {
        let mut buffer = FrameBuffer::new(5);
        
        // Add frames out of order
        buffer.add_frame(create_test_frame(2.0));
        buffer.add_frame(create_test_frame(1.0));
        buffer.add_frame(create_test_frame(4.0));
        buffer.add_frame(create_test_frame(3.0));
        
        // Test sequential access
        buffer.current_position = 0.0;
        let frame1 = buffer.next_frame().unwrap();
        assert_eq!(frame1.timestamp, 1.0);
        
        let frame2 = buffer.next_frame().unwrap();
        assert_eq!(frame2.timestamp, 2.0);
    }
    
    #[test]
    fn test_frame_buffer_seek() {
        let mut buffer = FrameBuffer::new(10);
        
        // Add several frames
        for i in 0..10 {
            buffer.add_frame(create_test_frame(i as f64));
        }
        
        // Seek to middle
        buffer.seek(5.0);
        assert_eq!(buffer.current_position, 5.0);
        
        let frame = buffer.current_frame().unwrap();
        assert_eq!(frame.timestamp, 5.0);
        
        // Next frame should be 6.0
        let next = buffer.next_frame().unwrap();
        assert_eq!(next.timestamp, 6.0);
    }
}