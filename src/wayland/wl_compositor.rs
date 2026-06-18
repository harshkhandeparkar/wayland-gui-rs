use std::io;

use super::WaylandComm;

pub mod opcodes {
    pub const CREATE_SERVICE: u16 = 0;
}

pub trait WlCompositor: WaylandComm {
    fn wl_compositor_create_surface(&mut self, wl_compositor_id: u32) -> io::Result<u32> {
        let new_id = self.get_new_id();

        self.send_req(
            wl_compositor_id,
            opcodes::CREATE_SERVICE,
            &new_id.to_ne_bytes(),
        )?;

        Ok(new_id)
    }
}

impl<T: WaylandComm> WlCompositor for T {}
