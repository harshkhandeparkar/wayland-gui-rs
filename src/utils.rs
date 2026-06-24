use std::{io, os::fd::AsRawFd};

use memfd::{FileSeal, Memfd, MemfdOptions};
use memmap2::{MmapMut, RemapOptions};

/// ARGB888 formatted double buffer wayland shared memory
pub struct WaylandShmDoubleBufARGB8888 {
    pub size: usize,
    pub height: u32,
    pub width: u32,
    pub stride: u32,

    pub memfd: Memfd,
    pub mmap: MmapMut,
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
        let size = (stride as usize) * (height as usize) * 2; // *2 since two buffers
        shm_file
            .set_len(size as u64)
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

        WaylandShmDoubleBufARGB8888 {
            size,
            stride,
            width,
            height,
            memfd,
            mmap,
        }
    }

    /// Resizes the shared memory buffer and returns the old and the new sizes in bytes
    /// Increases the size of the shared memory if the new size is more then the old size.
    /// Changes the stride, height, and width in either case.
    ///
    /// Returns the old and the new size of the shared memory buffer in bytes
    pub fn resize(&mut self, width: u32, height: u32) -> io::Result<(usize, usize)> {
        let stride = width * 4;
        let new_size = (stride as usize) * (height as usize) * 2;
        let old_size = self.size;

        if new_size > self.size {
            self.memfd.as_file().set_len(new_size as u64)?;

            unsafe {
                self.mmap
                    .remap(new_size, RemapOptions::new().may_move(true))?
            }

            self.size = new_size;
        }

        self.width = width;
        self.height = height;
        self.stride = stride;

        Ok((old_size, self.size))
    }

    pub fn get_raw_fd(&self) -> i32 {
        self.memfd.as_raw_fd()
    }
}
