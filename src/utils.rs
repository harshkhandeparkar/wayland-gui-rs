use std::{io, os::fd::AsRawFd};

use memfd::{FileSeal, Memfd, MemfdOptions};
use memmap2::MmapMut;

#[macro_export]
macro_rules! roundup_4 {
    ($x: expr) => {
        $x.div_ceil(4) * 4
    };
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
