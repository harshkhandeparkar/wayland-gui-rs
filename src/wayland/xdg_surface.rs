use super::WaylandComm;
use std::io;

pub mod opcodes {
    pub const DESTROY: u16 = 0;
    pub const ACK_CONFIGURE: u16 = 4;
    pub const GET_TOPLEVEL: u16 = 1;
}

pub mod events {
    pub const CONFIGURE: u16 = 0;
}

pub trait XdgSurface: WaylandComm {
    fn xdg_surface_destroy(&mut self, xdg_surface_id: u32) -> io::Result<()> {
        self.send_req(xdg_surface_id, opcodes::DESTROY, &[])?;

        Ok(())
    }

    fn xdg_surface_get_toplevel(&mut self, xdg_surface_id: u32) -> io::Result<u32> {
        let new_id = self.get_new_id();

        self.send_req(xdg_surface_id, opcodes::GET_TOPLEVEL, &new_id.to_ne_bytes())?;

        Ok(new_id)
    }

    fn xdg_surface_ack_configure(&mut self, xdg_surface_id: u32, serial: u32) -> io::Result<()> {
        self.send_req(
            xdg_surface_id,
            opcodes::ACK_CONFIGURE,
            &serial.to_ne_bytes(),
        )?;
        Ok(())
    }
}


impl<T: WaylandComm> XdgSurface for T {}
