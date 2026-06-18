use super::WaylandComm;
use std::{ffi::CString, io};

pub const OBJECT_ID: u32 = 1;

pub mod events {
    pub const ERROR: u16 = 0;
}

pub mod opcodes {
    pub const GET_REGISTRY: u16 = 1;
}

#[derive(Debug)]
pub struct WlDisplayError {
    pub object_id: u32,
    pub error_code: u32,
    pub msg: CString
}

pub trait WlDisplay: WaylandComm {
    /// Sends the `wl_display.get_registry` request
    fn wl_display_get_registry(&mut self) -> io::Result<u32> {
        let new_id = self.get_new_id();

        self.send_req(OBJECT_ID, opcodes::GET_REGISTRY, &new_id.to_ne_bytes())?;
        println!("wl_display@{}: wl_registry={}", OBJECT_ID, new_id);

        Ok(new_id)
    }
}

impl<T: WaylandComm> WlDisplay for T {}
