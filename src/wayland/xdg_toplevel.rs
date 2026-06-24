use super::WaylandComm;
use std::io;

pub mod opcodes {
    pub const DESTROY: u16 = 0;
}

pub mod events {
    pub const CONFIGURE: u16 = 0;
    pub const CLOSE: u16 = 1;
    pub const WM_CAPABILITIES: u16 = 3;
}

#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
#[repr(u32)]
#[derive(Debug)]
pub enum WM_CAPABILITIES {
    WINDOW_MENU = 1,
    MAXIMIZE = 2,
    FULLSCREEN = 3,
    MINIMIZE = 4,
}

impl TryFrom<u32> for WM_CAPABILITIES {
    type Error = String;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::WINDOW_MENU),
            2 => Ok(Self::MAXIMIZE),
            3 => Ok(Self::FULLSCREEN),
            4 => Ok(Self::MINIMIZE),
            _ => Err(format!("Invalid capability {value}")),
        }
    }
}

pub trait XdgToplevel: WaylandComm {
    fn xdg_toplevel_destroy(&mut self, xdg_toplevel_id: u32) -> io::Result<()> {
        self.send_req(xdg_toplevel_id, opcodes::DESTROY, &[])?;

        Ok(())
    }
}

impl<T: WaylandComm> XdgToplevel for T {}
