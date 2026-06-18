use super::WaylandComm;
use std::io;

pub mod events {
    pub const RELEASE: u16 = 0;
}

pub mod opcodes {
    pub const DESTROY: u16 = 0;
}

pub trait WlBuffer: WaylandComm {
    fn wl_buffer_destroy(&mut self, wl_buffer_id: u32) -> io::Result<()> {
        self.send_req(wl_buffer_id, opcodes::DESTROY, &[])?;

        Ok(())
    }
}

impl<T: WaylandComm> WlBuffer for T {}
