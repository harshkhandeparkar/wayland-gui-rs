use nix::sys::socket::{ControlMessage, MsgFlags, sendmsg};

use crate::roundup_4;
use std::{
    env,
    ffi::{CStr, CString},
    io::{self, IoSlice, Read, Write},
    os::{fd::AsRawFd, unix::net::UnixStream},
    path::PathBuf,
};

/// Returns the string size, padded size (including the 32bit size number), the string, and a new slice/buffer pointing to the end of
/// the string
pub fn read_wl_string(buf: &[u8]) -> (usize, usize, &CStr, &[u8]) {
    let str_size = u32::from_ne_bytes(buf[0..4].try_into().unwrap());
    let str_size = usize::try_from(str_size).unwrap();
    let padded_str_size = roundup_4!(str_size) + 4;

    assert!(
        padded_str_size <= buf.len(),
        "Arguments buffer smaller than the announced padded string size."
    );

    let parsed_str = CStr::from_bytes_with_nul(&buf[4..4 + str_size]).unwrap();

    (
        str_size,
        padded_str_size,
        parsed_str,
        &buf[padded_str_size..],
    )
}

/// Packs a string into a Wayland protocol-compatible `string` bytes and writes to a buffer
pub fn write_wl_string(string: &CStr, buf: &mut Vec<u8>) {
    let mut string_bytes = string.to_bytes_with_nul().to_vec();
    let str_len = string_bytes.len();
    let padded_len = roundup_4!(str_len);

    // Null pad the bytes
    string_bytes.resize(padded_len, 0);

    buf.extend_from_slice(&u32::try_from(str_len).unwrap().to_ne_bytes());
    buf.extend_from_slice(&string_bytes);
}

/// Reads a u32 from a &[u8] buffer, returns the value and returns a new buffer/slice with the
/// offset moved 4 bytes ahead.
pub fn read_wl_u32(buf: &[u8]) -> (u32, &[u8]) {
    let val = u32::from_ne_bytes(buf[0..4].try_into().unwrap());

    (val, &buf[4..])
}

/// Reads a u16 from a &[u8] buffer, returns the value and returns a new buffer/slice with the
/// offset moved 2 bytes ahead.
pub fn read_wl_u16(buf: &[u8]) -> (u16, &[u8]) {
    let val = u16::from_ne_bytes(buf[0..2].try_into().unwrap());

    (val, &buf[2..])
}

/// Reads a Wayland array (of bytes), returns the array as a u32 slice and returns a new
/// buffer/slice with the offset moved to the end of the array.
pub fn read_wl_array(buf: &[u8]) -> (&[u32], &[u8]) {
    let (size, buf) = read_wl_u32(buf);
    let size = usize::try_from(size).unwrap();
    let padded_size = roundup_4!(size);

    let (prefix, arr, suffix) = unsafe { &buf[0..size].align_to::<u32>() };

    assert!(
        prefix.is_empty() && suffix.is_empty(),
        "Array is not aligned."
    );

    (arr, &buf[padded_size..])
}

pub const WL_HEADER_SIZE: usize = 8;
#[derive(Debug)]
pub struct WaylandHeader {
    pub object_id: u32,
    pub opcode: u16,
    pub msg_size: usize,
}
/// parses the [WaylandHeader] from a message buffer (&[u8] slice) and returns the header and a new
/// slice with an offset that points at the arguments
pub fn parse_wl_header(buf: &[u8]) -> (WaylandHeader, &[u8]) {
    assert!(buf.len() >= 8, "Buffer size less than header size.");

    let (object_id, buf) = read_wl_u32(buf);
    let (opcode, buf) = read_wl_u16(buf);
    let (msg_size, buf) = read_wl_u16(buf);

    let msg_size = usize::from(msg_size);

    let header = WaylandHeader {
        object_id,
        opcode,
        msg_size,
    };

    (header, buf)
}

#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    pub unique_name: u32,
    pub name: CString,
    pub version: u32,
}

impl InterfaceInfo {
    pub fn from_registry_global_event(args: &[u8]) -> Self {
        let (unique_name, args) = read_wl_u32(args);
        let (_, _, name, args) = read_wl_string(args);

        let (version, _args) = read_wl_u32(args);

        Self {
            unique_name,
            name: name.to_owned(),
            version,
        }
    }
}

pub trait WaylandComm {
    /// Sends a request (message) to the server
    fn send_req(&mut self, object_id: u32, opcode: u16, args: &[u8]) -> io::Result<()>;

    /// Sends a request (message) to the server along with a file descriptor in the ancillary data
    fn send_req_with_fd(
        &mut self,
        object_id: u32,
        opcode: u16,
        args: &[u8],
        raw_fd: i32,
    ) -> io::Result<usize>;

    /// Packs the header and arguments of a wayland request (message) into a single u8 vec/buffer.
    /// Returns the Vec<u8> buffer and the message size in bytes
    fn pack_req_data(&self, object_id: u32, opcode: u16, args: &[u8]) -> (Vec<u8>, usize);

    /// Reads incoming events (messages) into a buffer
    fn read_events(&mut self) -> io::Result<()>;

    /// Returns a `MsgBufReader` that can be iterated over to handle events.
    /// Each item of the iterator is a tuple of (header, args) where args is a byte slice of arguments
    ///
    /// Once all events are handled, consume the `MsgBufReader` with `.finish()` and pass the
    /// remainder to `.events_handled()`.
    fn reader(&self) -> MsgBufReader<'_>;

    /// To be called when events handling is finished. Takes an offset to any remainder bytes and moves those back to the start of
    /// the buffer, and can start reading the next set of bytes
    fn reset_buffer(&mut self, remainder_offset: usize);

    /// Returns a sequential ID to create a new wayland object
    fn get_new_id(&mut self) -> u32;
}

pub struct MsgBufReader<'a> {
    remainder: &'a [u8],
    read_offset: usize,
}

impl<'a> MsgBufReader<'a> {
    /// Returns the offset of the remainder bytes (any unparsed/unread events) and consumes the struct
    pub fn finish(self) -> usize {
        self.read_offset
    }
}

impl<'a> Iterator for MsgBufReader<'a> {
    /// Returns the header, reference to the arguments slice, and the size of the arguments slice
    /// (in bytes)
    type Item = (WaylandHeader, &'a [u8], usize);

    fn next(&mut self) -> Option<Self::Item> {
        if self.remainder.is_empty() {
            None
        } else if self.remainder.len() < WL_HEADER_SIZE {
            // Header not fully read, end iteration
            None
        } else {
            let (header, remaining) = parse_wl_header(self.remainder);
            let args_size = header.msg_size - WL_HEADER_SIZE;

            if args_size > remaining.len() {
                // Message size is more than the number of bytes read.
                // More bytes can be read, end iteration

                None
            } else {
                // Successfully read everything
                // Update the remainder, and return the current event
                let args = &remaining[..args_size];

                self.remainder = &remaining[args_size..];
                self.read_offset += header.msg_size;

                Some((header, args, args_size))
            }
        }
    }
}

// Handles wayland socket communication
pub struct WaylandSocket {
    sock: UnixStream,
    last_obj_id: u32,

    msg_buf: [u8; 4096],
    write_offset: usize,
}

impl WaylandComm for WaylandSocket {
    fn pack_req_data(&self, object_id: u32, opcode: u16, args: &[u8]) -> (Vec<u8>, usize) {
        // Size of header + arguments
        let msg_size = WL_HEADER_SIZE + args.len();
        let msg_size_u16 = u16::try_from(msg_size).unwrap();

        let buf = [
            &object_id.to_ne_bytes()[..],
            &opcode.to_ne_bytes()[..],
            &msg_size_u16.to_ne_bytes()[..],
            args,
        ]
        .concat();

        assert!(
            msg_size == buf.len(),
            "Calculated message_size does not match the buffer size."
        );

        (buf, msg_size)
    }

    fn send_req(&mut self, object_id: u32, opcode: u16, args: &[u8]) -> io::Result<()> {
        let (msg_data, msg_size) = self.pack_req_data(object_id, opcode, args);

        println!(
            "Sending message of size {msg_size} bytes. Object ID: {object_id}. Opcode: {opcode}."
        );

        self.sock.write_all(&msg_data)
    }

    fn send_req_with_fd(
        &mut self,
        object_id: u32,
        opcode: u16,
        args: &[u8],
        raw_fd: i32,
    ) -> io::Result<usize> {
        let cmsg = ControlMessage::ScmRights(&[raw_fd]);
        let (msg_data, _) = self.pack_req_data(object_id, opcode, args);

        let sock_fd = self.sock.as_raw_fd();

        Ok(sendmsg::<()>(
            sock_fd,
            &[IoSlice::new(&msg_data)],
            &[cmsg],
            MsgFlags::empty(),
            None,
        )?)
    }

    fn read_events(&mut self) -> io::Result<()> {
        let read_bytes = match self.sock.read(&mut self.msg_buf[self.write_offset..]) {
            Ok(bytes) => Ok(bytes),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(0),
            Err(e) => Err(e),
        }?;

        self.write_offset += read_bytes;

        Ok(())
    }

    fn reader(&self) -> MsgBufReader<'_> {
        MsgBufReader {
            remainder: &self.msg_buf[..self.write_offset],
            read_offset: 0,
        }
    }

    fn reset_buffer(&mut self, remainder_offset: usize) {
        self.msg_buf
            .copy_within(remainder_offset..self.write_offset, 0);

        self.write_offset -= remainder_offset;
    }

    fn get_new_id(&mut self) -> u32 {
        self.last_obj_id += 1;
        self.last_obj_id
    }
}

impl WaylandSocket {
    pub fn new() -> Self {
        let xdg_runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap();

        let wayland_display = env::var("WAYLAND_DISPLAY").unwrap_or("wayland-0".into());

        let socket_path = PathBuf::new();
        let socket_path = socket_path.join(xdg_runtime_dir).join(wayland_display);

        let sock = UnixStream::connect(&socket_path).unwrap();
        sock.set_nonblocking(true).unwrap();

        println!("connected to wayland socket {:?}", socket_path);

        Self {
            sock,
            last_obj_id: 1,
            msg_buf: [0_u8; 4096],
            write_offset: 0,
        }
    }
}

pub mod wl_buffer;
pub mod wl_compositor;
pub mod wl_display;
pub mod wl_registry;
pub mod wl_shm;
pub mod wl_shm_pool;
pub mod wl_surface;
pub mod xdg_surface;
pub mod xdg_toplevel;
pub mod xdg_wm_base;
