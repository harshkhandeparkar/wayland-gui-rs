use super::WaylandComm;
use std::io;

pub mod opcodes {
    pub const DESTROY: u16 = 0;
    pub const GET_XDG_SURFACE: u16 = 2;
    pub const PONG: u16 = 3;
}

pub mod events {
    pub const PING: u16 = 0;
}

pub trait XdgWmBase: WaylandComm {
    fn xdg_wm_base_get_xdg_surface(
        &mut self,
        xdg_wm_base_id: u32,
        wl_surface_id: u32,
    ) -> io::Result<u32> {
        let new_id = self.get_new_id();

        self.send_req(
            xdg_wm_base_id,
            opcodes::GET_XDG_SURFACE,
            &[new_id.to_ne_bytes(), wl_surface_id.to_ne_bytes()].concat(),
        )?;

        Ok(new_id)
    }

    fn xdg_wm_base_destroy(&mut self, xdg_wm_base_id: u32) -> io::Result<()> {
        self.send_req(xdg_wm_base_id, opcodes::DESTROY, &[])?;
        Ok(())
    }

    fn xdg_wm_base_pong(&mut self, xdg_wm_base_id: u32, serial: u32) -> io::Result<()> {
        self.send_req(xdg_wm_base_id, opcodes::PONG, &serial.to_ne_bytes())?;

        Ok(())
    }
}

impl<T: WaylandComm> XdgWmBase for T {}
