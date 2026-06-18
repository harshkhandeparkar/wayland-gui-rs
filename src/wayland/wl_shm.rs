use super::WaylandComm;
use std::io;

pub mod events {
    pub const FORMAT: u16 = 0;
}

pub mod opcodes {
    pub const CREATE_POOL: u16 = 0;
    pub const RELEASE: u16 = 1;
}

pub const FORMAT_ARGB888: u32 = 0;

pub trait WlShm: WaylandComm {
    fn wl_shm_release(&mut self, wl_shm_id: u32) -> io::Result<()> {
        self.send_req(wl_shm_id, opcodes::RELEASE, &[])?;
        Ok(())
    }

    fn wl_shm_create_pool(&mut self, wl_shm_id: u32, size: u32, raw_fd: i32) -> io::Result<u32> {
        let new_id = self.get_new_id();
        let args = &[new_id.to_ne_bytes(), size.to_ne_bytes()].concat();

        self.send_req_with_fd(wl_shm_id, opcodes::CREATE_POOL, args, raw_fd)?;

        Ok(new_id)
    }
}

impl<T: WaylandComm> WlShm for T {}
