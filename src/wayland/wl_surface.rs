use super::WaylandComm;
use std::io;

pub mod opcodes {
    pub const DESTROY: u16 = 0;
    pub const ATTACH: u16 = 1;
    pub const COMMIT: u16 = 6;
    pub const DAMAGE_BUFFER: u16 = 9;
}

pub trait WlSurface: WaylandComm {
    fn wl_surface_destroy(&mut self, wl_surface_id: u32) -> io::Result<()> {
        self.send_req(wl_surface_id, opcodes::DESTROY, &[])?;

        Ok(())
    }

    fn wl_surface_attach(&mut self, wl_surface_id: u32, buf_id: u32) -> io::Result<()> {
        let args = &[
            buf_id.to_ne_bytes(),
            [0_u8; 4], // x offset (surface local coords); In version 5+, this MUST be set to 0
            [0_u8; 4], // y offset (surface local coords); In version 5+, this MUST be set to 0
        ]
        .concat();

        self.send_req(wl_surface_id, opcodes::ATTACH, args)?;
        Ok(())
    }

    fn wl_surface_commit(&mut self, wl_surface_id: u32) -> io::Result<()> {
        self.send_req(wl_surface_id, opcodes::COMMIT, &[])?;
        Ok(())
    }

    fn wl_surface_damage_buffer(
        &mut self,
        wl_surface_id: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> io::Result<()> {
        self.send_req(
            wl_surface_id,
            opcodes::DAMAGE_BUFFER,
            &[
                x.to_ne_bytes(),
                y.to_ne_bytes(),
                width.to_ne_bytes(),
                height.to_ne_bytes(),
            ]
            .concat(),
        )?;
        Ok(())
    }
}


impl<T: WaylandComm> WlSurface for T {}
