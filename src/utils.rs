use std::{io, os::fd::AsRawFd};

use memfd::{FileSeal, Memfd, MemfdOptions};
use memmap2::MmapMut;

/// A (heap allocated) byte buffer aligned to multiples of 4.
/// Internally stored as a Vec<u32> on the heap and can be cast into u8 slices.
/// This is a very hacky way of doing this and there are probably better ways of doing it. The best
/// way is probably to custom allocate some bytes and have a custom type that is then converted into
/// a Vec<u32>. There's also the `aligned-vec` crate.
pub struct U32AlignedBuf {
    buf: Vec<u32>,
    /// Size of the buffer in bytes
    size: usize,
    /// Length of the underlying Vec<u32> buffer
    buf_size: usize,
}

impl U32AlignedBuf {
    /// Creates an aligned buffer of size N bytes (N must be a multiple of 4)
    pub fn with_size(size: usize) -> Self {
        assert!(
            size.is_multiple_of(4),
            "Size in bytes must be a multiple of 4"
        );

        let buf_size = size / 4;

        Self {
            buf: Vec::with_capacity(buf_size),
            buf_size,
            size,
        }
    }

    /// Sets the written length of the underlying buffer in bytes.
    /// Since the underlying buffer is a Vec<u32>, this sets its length in terms of u32 (4 bytes) to
    /// the next closest multiple of 4. Panics if the size is bigger than its length can be.
    ///
    /// This function has to be called if the underlying buffer is written to using the `bytes_mut()` method.
    pub fn set_len(&mut self, bytes: usize) {
        let new_len = bytes.div_ceil(4);

        assert!(
            new_len <= self.buf_size,
            "The length is bigger than the size of the buffer."
        );

        unsafe {
            self.buf.set_len(new_len);
        }
    }

    /// Returns the underlying buffer as an immutable byte slice. **NOTE**: This returns a slice to
    /// only the written data (set using the `set_len()` function)
    ///
    /// This is safe because a Vec<u32> is guaranteed to be a continuous heap allocation
    /// If the vector is not initialized with_capacity(), each time it grows, a new heap area is
    /// allocated and it copies all data.
    pub fn bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.buf.as_ptr() as *const u8, self.buf.len() * 4) }
    }

    /// Returns the underlying buffer as a mutable byte slice. **IMPORTANT**: If the underlying
    /// buffer is written to using the byte slice, then the `set_len()` method HAS to be called to
    /// set the `len()` of the underlying Vec<u32> buffer. **NOTE**: This returns a slice to ALL of
    /// the underlying data, not just the parts that have been written to.
    ///
    /// See bytes() for why this is safe.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.buf.as_mut_ptr() as *mut u8, self.size) }
    }

    /// Returns a u32 slice for the underlying buffer. **NOTE**: This only returns a slice to the
    /// written data (set using `set_len`). i.e., it returns a slice to the underlying Vec<u32>.
    pub fn u32(&self) -> &[u32] {
        self.buf.as_slice()
    }

    /// Returns a mutable u32 slice for the underlying buffer. **NOTE**: This only returns a slice to the
    /// written data (set using `set_len`). i.e., it returns a slice to the underlying Vec<u32>.
    pub fn u32_mut(&mut self) -> &mut [u32] {
        self.buf.as_mut_slice()
    }

    /// Returns the underlying Vec<u32> buffer
    pub fn into_u32_vec(self) -> Vec<u32> {
        self.buf
    }
}

/// ARGB888 formatted double buffer wayland shared memory
pub struct WaylandShmDoubleBufARGB8888 {
    pub size: u64,
    pub height: u32,
    pub width: u32,
    pub stride: u32,

    pub memfd: Memfd,
    pub mmap: MmapMut,
    pub raw_fd: i32,
}

impl WaylandShmDoubleBufARGB8888 {
    /// Creates a shared memory (double) buffer and returns the `MemFd` object, pointer (mmap), and the raw fd
    pub fn new(width: u32, height: u32) -> Self {
        let memfd = MemfdOptions::new()
            .close_on_exec(true)
            .allow_sealing(true)
            .create("wayland_shm_buf")
            .expect("Failed to create a `memfd` file.");

        let shm_file = memfd.as_file();

        let stride = width * 4;
        let size = (stride as u64) * (height as u64) * 2; // *2 since two buffers
        shm_file
            .set_len(size)
            .expect("Failed to allocate size for the `memfd` shared buffer.");

        let mut seals = memfd
            .seals()
            .expect("Failed to read seals on the `memfd` shared buffer.");

        seals.insert(FileSeal::SealShrink);
        // seals.insert(FileSeal::SealGrow);
        seals.insert(FileSeal::SealSeal);

        memfd
            .add_seals(&seals)
            .expect("Failed to apply memory seals to the `memfd` shared  buffer.");

        let mmap = unsafe {
            MmapMut::map_mut(shm_file).expect("Failed to `mmap` the `memfd` shared buffer.")
        };

        let raw_fd = memfd.as_raw_fd();

        WaylandShmDoubleBufARGB8888 {
            size,
            stride,
            width,
            height,
            memfd,
            mmap,
            raw_fd,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) -> io::Result<()> {
        let stride = width * 4;
        let new_size = (stride as u64) * (height as u64) * 2;

        if new_size < self.size {
            self.memfd.as_file().set_len(new_size)?;

            self.width = width;
            self.height = height;
            self.stride = stride;
            self.size = new_size;
        }

        Ok(())
    }
}
