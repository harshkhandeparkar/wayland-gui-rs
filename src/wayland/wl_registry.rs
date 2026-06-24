use crate::wayland::InterfaceInfo;

use super::{WaylandComm, write_wl_string};
use std::io;

pub mod events {
    pub const GLOBAL: u16 = 0;
}

pub mod opcodes {
    pub const BIND: u16 = 0;
}

pub trait WlRegistry: WaylandComm {
    /// Sends the `wl_registry.bind` request
    fn wl_registry_bind(
        &mut self,
        wl_registry_id: u32,
        interface: &InterfaceInfo,
    ) -> io::Result<u32> {
        // `name` uint argument
        let mut args_buf = interface.unique_name.to_ne_bytes().to_vec();

        // `id` new_id argument which includes the interface name, version, and then the new id
        write_wl_string(interface.name.as_ref(), &mut args_buf);
        args_buf.extend_from_slice(interface.version.to_ne_bytes().as_ref());

        let new_id = self.get_new_id();
        args_buf.extend_from_slice(&new_id.to_ne_bytes());

        self.send_req(wl_registry_id, opcodes::BIND, &args_buf)?;

        Ok(new_id)
    }
}

impl<T: WaylandComm> WlRegistry for T {}
