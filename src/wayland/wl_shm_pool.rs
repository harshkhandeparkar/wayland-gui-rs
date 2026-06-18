use super::WaylandComm;
use std::io;
pub mod opcodes {
    pub const CREATE_BUFFER: u16 = 0;
    pub const DESTROY: u16 = 1;
}

pub trait WlShmPool: WaylandComm {
    fn wl_shm_pool_destroy(&mut self, wl_shm_pool_id: u32) -> io::Result<()> {
        self.send_req(wl_shm_pool_id, opcodes::DESTROY, &[])?;

        Ok(())
    }

    fn wl_shm_pool_create_buffer(
        &mut self,
        wl_shm_pool_id: u32,
        offset: u32,
        width: u32,
        height: u32,
        stride: u32,
        format: u32,
    ) -> io::Result<u32> {
        let new_id = self.get_new_id();

        let args = &[
            new_id.to_ne_bytes(),
            offset.to_ne_bytes(),
            width.to_ne_bytes(),
            height.to_ne_bytes(),
            stride.to_ne_bytes(),
            format.to_ne_bytes(),
        ]
        .concat();

        self.send_req(wl_shm_pool_id, opcodes::CREATE_BUFFER, args)?;

        Ok(new_id)
    }
}

impl<T: WaylandComm> WlShmPool for T {}
