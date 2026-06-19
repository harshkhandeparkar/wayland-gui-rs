use std::collections::HashMap;
use std::error::Error;
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{self, BufReader};
use std::process::exit;

use crate::utils::{U32AlignedBuf, WaylandShmDoubleBufARGB8888};

mod utils;
mod wayland;

use crate::wayland::wl_buffer::WlBuffer;
use crate::wayland::wl_compositor::WlCompositor;
use crate::wayland::wl_display::{WlDisplay, WlDisplayError};
use crate::wayland::wl_registry::WlRegistry;
use crate::wayland::wl_shm::WlShm;
use crate::wayland::wl_shm_pool::WlShmPool;
use crate::wayland::wl_surface::WlSurface;
use crate::wayland::xdg_surface::XdgSurface;
use crate::wayland::xdg_toplevel::XdgToplevel;
use crate::wayland::xdg_wm_base::XdgWmBase;
use crate::wayland::*;

#[derive(Default)]
struct InterfaceIds {
    wl_registry: u32,
    wl_shm: u32,
    wl_shm_pool: u32,

    wl_compositor: u32,
    wl_surface: u32,

    wl_buffer_1: u32,
    wl_buffer_2: u32,

    xdg_wm_base: u32,
    xdg_surface: u32,
    xdg_toplevel: u32,
}

struct SurfaceBuffers {
    /// Width of a single buffer in pixels
    width: u32,
    /// Height of a single buffer in pixels
    height: u32,

    buf_1_id: u32,
    /// Whether the first buffer has been released and ready for drawing
    buf_1_drawable: bool,

    buf_2_id: u32,
    /// Whether the second buffer has been released and ready for drawing
    buf_2_drawable: bool,

    /// Whether the first buffer is the front buffer (else, it is the back buffer)
    first_buf_front: bool,
    shm: WaylandShmDoubleBufARGB8888,
}

impl SurfaceBuffers {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,

            buf_1_id: 0,
            buf_1_drawable: true,
            buf_2_id: 0,
            buf_2_drawable: true,

            first_buf_front: true,
            shm: WaylandShmDoubleBufARGB8888::new(width, height),
        }
    }

    /// Sets the IDs for the associated `wl_buffer`s
    pub fn set_ids(&mut self, buf_1_id: u32, buf_2_id: u32) {
        self.buf_1_id = buf_1_id;
        self.buf_2_id = buf_2_id;
    }

    /// Returns a mutable reference to the draw buffer (back buffer)
    pub fn get_draw_buffer_mut(&mut self) -> &mut [u32] {
        if self.first_buf_front {
            if self.buf_2_drawable {
                self.get_pixels_mut().1
            } else {
                panic!("Trying to draw to unreleased buffer id {}", self.buf_2_id);
            }
        } else {
            if self.buf_1_drawable {
                self.get_pixels_mut().0
            } else {
                panic!("Trying to draw to unreleased buffer id {}", self.buf_1_id);
            }
        }
    }

    /// Brings the back buffer to the front for and returns it `wl_buffer` id for attaching to the surface
    pub fn commit_draw(&mut self) -> u32 {
        self.switch_bufs()
    }

    /// Switches the front and back buffers and returns the id to the front buffer
    fn switch_bufs(&mut self) -> u32 {
        self.first_buf_front = !self.first_buf_front;

        if self.first_buf_front {
            self.buf_1_drawable = false;
            self.buf_1_id
        } else {
            self.buf_2_drawable = false;
            self.buf_2_id
        }
    }

    pub fn release_buf(&mut self, buf_id: u32) {
        if buf_id == self.buf_1_id {
            self.buf_1_drawable = true;
        } else {
            self.buf_2_drawable = true;
        }
    }

    /// Returns a mutable slices to both the buffers in the shared memory
    fn get_pixels_mut(&mut self) -> (&mut [u32], &mut [u32]) {
        let end = (self.shm.size as usize) / 2;

        let (buf_1, buf_2) = self.shm.mmap.split_at_mut(end);

        let (prefix_1, buf_1, suffix_1) = unsafe { buf_1.align_to_mut() };
        let (prefix_2, buf_2, suffix_2) = unsafe { buf_2.align_to_mut() };

        assert!(
            prefix_1.is_empty() && suffix_1.is_empty(),
            "Bytes of the first shared buffer not aligned to u32."
        );
        assert!(
            prefix_2.is_empty() && suffix_2.is_empty(),
            "Bytes of the second shared buffer not aligned to u32."
        );

        (buf_1, buf_2)
    }
}

#[derive(Debug)]
enum ClientStatus {
    Connected,
    GotRegistry,
    ReadingGlobalInterfaces,
    BindingInterfaces,
    AwaitingFormats,
    ReadingFormats,
    CreatingSurface,
    AwaitingInitialConfigure,
    GotInitialConfigure,
    CreatingBuffers,
    Drawing,
    Resizing,
    Idle,
    Closing,
    Exiting,
}

#[derive(Debug)]
struct ClientState {
    status: ClientStatus,
    pending_ping: Option<u32>,
    pending_configure: Option<u32>,
    pending_errors: Vec<WlDisplayError>,
}

struct WaylandClient {
    sock: WaylandSocket,

    img_argb8: Vec<u32>,
    state: ClientState,

    advertised_interfaces: HashMap<CString, InterfaceInfo>,
    interfaces: InterfaceIds,

    window_width: u32,
    window_height: u32,

    draw_height: u32,
    draw_width: u32,

    surface_buffers: Option<SurfaceBuffers>,
}

impl WaylandClient {
    fn new(image_path: &str) -> Self {
        let sock = WaylandSocket::new();

        // Reading the image
        let decoder = png::Decoder::new(BufReader::new(File::open(image_path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let image_info = reader.info();

        let img_width = image_info.width;
        let img_height = image_info.height;

        let img_buf_size = (img_width as usize) * (img_height as usize) * 4;
        let mut img_buf = U32AlignedBuf::with_size(img_buf_size);

        reader.next_frame(img_buf.bytes_mut()).unwrap();

        // It is completely utilized
        img_buf.set_len(img_buf_size);

        let (chunks, remainder) = img_buf.bytes_mut().as_chunks_mut::<4>();
        assert!(
            remainder.is_empty(),
            "Pixels buffer not in multiples of 4 bytes."
        );

        for chunk in chunks {
            let [r, g, b, a] = chunk;

            let r_u32 = *r as u32;
            let g_u32 = *g as u32;
            let b_u32 = *b as u32;
            let a_u32 = *a as u32;

            // Alpha must be premultiplied with the RGB channels
            *r = ((r_u32 * a_u32) >> 8) as u8;
            *g = ((g_u32 * a_u32) >> 8) as u8;
            *b = ((b_u32 * a_u32) >> 8) as u8;

            // Since it is little endian, the format is actually BGRA
            // The lowest byte is B, and it is stored at the lowest address (i.e., first byte)
            chunk[0..3].reverse();
        }

        Self {
            sock,
            img_argb8: img_buf.into_u32_vec(),
            state: ClientState {
                status: ClientStatus::Connected,
                pending_ping: None,
                pending_configure: None,
                pending_errors: Vec::new(),
            },

            interfaces: InterfaceIds::default(),
            advertised_interfaces: HashMap::new(),

            window_width: img_width,
            window_height: img_height,
            draw_width: img_width,
            draw_height: img_height,

            surface_buffers: None,
        }
    }

    fn bind_interface(&mut self, interface_name: &CStr) -> io::Result<Option<u32>> {
        if let Some(interface) = self.advertised_interfaces.get(interface_name) {
            let new_id = self
                .sock
                .wl_registry_bind(self.interfaces.wl_registry, interface)?;

            println!("Bound the {:?} interface. ID: {new_id}", interface_name);

            Ok(Some(new_id))
        } else {
            println!(
                "Could not bind the {:?} interface. Interface is not advertised.",
                interface_name
            );
            Ok(None)
        }
    }

    fn bind_interfaces(&mut self) -> io::Result<()> {
        if let Some(id) = self.bind_interface(c"wl_shm")? {
            self.interfaces.wl_shm = id;
        }
        if let Some(id) = self.bind_interface(c"xdg_wm_base")? {
            self.interfaces.xdg_wm_base = id;
        }
        if let Some(id) = self.bind_interface(c"wl_compositor")? {
            self.interfaces.wl_compositor = id;
        }

        Ok(())
    }

    fn create_buffers(&mut self) -> io::Result<()> {
        println!(
            "Creating window of w={} h={}",
            self.window_width, self.window_height
        );
        let mut surface_bufs = SurfaceBuffers::new(self.window_width, self.window_width);

        self.interfaces.wl_shm_pool = self.sock.wl_shm_create_pool(
            self.interfaces.wl_shm,
            surface_bufs.shm.size as u32,
            surface_bufs.shm.raw_fd,
        )?;

        println!(
            "Successfully created a `wl_shm_pool`. ID={}",
            self.interfaces.wl_shm_pool
        );

        // Creating buffers
        self.interfaces.wl_buffer_1 = self.sock.wl_shm_pool_create_buffer(
            self.interfaces.wl_shm_pool,
            0,
            surface_bufs.width,
            surface_bufs.height,
            surface_bufs.shm.stride,
            wl_shm::FORMAT_ARGB888,
        )?;
        self.interfaces.wl_buffer_2 = self.sock.wl_shm_pool_create_buffer(
            self.interfaces.wl_shm_pool,
            (surface_bufs.shm.size / 2) as u32,
            surface_bufs.width,
            surface_bufs.height,
            surface_bufs.shm.stride,
            wl_shm::FORMAT_ARGB888,
        )?;
        println!(
            "Successfully created two `wl_buffer`s. ID={} and {}",
            self.interfaces.wl_buffer_1, self.interfaces.wl_buffer_2
        );

        surface_bufs.set_ids(self.interfaces.wl_buffer_1, self.interfaces.wl_buffer_2);

        self.surface_buffers = surface_bufs.into();

        Ok(())
    }

    fn create_surface(&mut self) -> io::Result<()> {
        self.interfaces.wl_surface = self
            .sock
            .wl_compositor_create_surface(self.interfaces.wl_compositor)?;
        println!("Created `wl_surface`. ID: {}.", self.interfaces.wl_surface);

        self.interfaces.xdg_surface = self
            .sock
            .xdg_wm_base_get_xdg_surface(self.interfaces.xdg_wm_base, self.interfaces.wl_surface)?;
        println!(
            "Created `xdg_surface`. ID: {}.",
            self.interfaces.xdg_surface
        );

        self.interfaces.xdg_toplevel = self
            .sock
            .xdg_surface_get_toplevel(self.interfaces.xdg_surface)?;
        println!(
            "Created `xdg_toplevel`. ID: {}.",
            self.interfaces.xdg_toplevel
        );

        self.sock.wl_surface_commit(self.interfaces.wl_surface)?;
        println!("Successfully committed the `wl_surface`.");

        Ok(())
    }

    fn draw(&mut self) -> io::Result<()> {
        let image = self.img_argb8.as_slice();

        if let Some(surface_buf) = self.surface_buffers.as_mut() {
            let img_row_stride = self.draw_width as usize;

            let buf_row_stride = surface_buf.width as usize;
            let pixels = surface_buf.get_draw_buffer_mut();

            let x_offset = ((self.window_width - self.draw_width) / 2) as usize;
            let y_offset = ((self.window_height - self.draw_height) / 2) as usize;
            println!("drawing with offsets x = {x_offset} y = {y_offset}");

            for row in 0..(self.draw_height as usize) {
                let img_row_start = row * img_row_stride;
                let img_row = &image[img_row_start..img_row_start + img_row_stride];

                let pixels_row_start = (row + y_offset) * buf_row_stride + x_offset;
                let pixels_row = &mut pixels[pixels_row_start..pixels_row_start + img_row_stride];

                pixels_row.copy_from_slice(img_row);
            }

            let attach_buf_id = surface_buf.commit_draw();

            self.sock
                .wl_surface_attach(self.interfaces.wl_surface, attach_buf_id)?;
            println!("Successfully attached front buffer id {attach_buf_id} to the `wl_surface`.");

            self.sock.wl_surface_damage_buffer(
                self.interfaces.wl_surface,
                x_offset as i32,
                y_offset as i32,
                self.draw_width as i32,
                self.draw_height as i32,
            )?;
            println!("Successfully damaged the `wl_surface`.");

            self.sock.wl_surface_commit(self.interfaces.wl_surface)?;
            println!("Successfully committed to the `wl_surface`.");
        } else {
            panic!("Draw buffer not found.");
        }

        Ok(())
    }

    fn read_events(&mut self) -> io::Result<()> {
        self.sock.read_events()
    }

    /// Handles unhandled events in the buffer
    /// Returns true if any events were handled during this iteration or if any events are pending
    /// (not fully read). The output boolean is treated as a state that represents whether event
    /// handling is in progress.
    fn handle_events(&mut self) -> bool {
        // Whether any events were handled
        let mut events_handled = false;

        let remainder_offset = {
            let mut msg_reader = self.sock.reader();

            for (header, args, args_size) in msg_reader.by_ref() {
                println!("Header: {:?}", header);
                events_handled = true;

                if header.object_id == self.interfaces.wl_registry
                    && header.opcode == wl_registry::events::GLOBAL
                {
                    println!("Received `Announce Global Object` Event:");

                    let interface = InterfaceInfo::from_registry_global_event(args);

                    println!("Advertised interface: {:?}", interface);

                    self.advertised_interfaces
                        .insert(interface.name.clone(), interface);

                    self.state.status = ClientStatus::ReadingGlobalInterfaces;
                } else if header.object_id == wl_display::OBJECT_ID
                    && header.opcode == wl_display::events::ERROR
                {
                    println!("Received `Error` Event:");

                    let (object_id, args) = read_wl_u32(args);
                    let (error_code, args) = read_wl_u32(args);

                    let (_, msg_str_padded_size, msg, _args) = read_wl_string(args);

                    let err = WlDisplayError {
                        object_id,
                        error_code,
                        msg: msg.to_owned(),
                    };

                    assert!(
                        4 + 4 + msg_str_padded_size == args_size,
                        "Padded string size + argument size != Announced argument size"
                    );

                    println!("Received error: {err:?}");
                    self.state.pending_errors.push(err);
                } else if header.object_id == self.interfaces.wl_shm
                    && header.opcode == wl_shm::events::FORMAT
                {
                    // Currently list of formats is not required as ARGB8888 is guaranteed to exist
                    self.state.status = ClientStatus::ReadingFormats;
                } else if header.object_id == self.interfaces.xdg_wm_base
                    && header.opcode == xdg_wm_base::events::PING
                {
                    let (serial, _) = read_wl_u32(args);

                    println!("Received a `Ping` event. Serial = {serial}. Sending a Pong.");

                    self.state.pending_ping = Some(serial);
                } else if header.object_id == self.interfaces.xdg_surface
                    && header.opcode == xdg_surface::events::CONFIGURE
                {
                    let (serial, _) = read_wl_u32(args);

                    println!(
                        "Received a `xdg_surface.configure` event. Serial = {serial}. Sending an ACK."
                    );

                    self.state.pending_configure = Some(serial);
                } else if header.object_id == self.interfaces.xdg_toplevel {
                    if header.opcode == xdg_toplevel::events::WM_CAPABILITIES {
                        let (capabilities, _args) = read_wl_array(args);
                        let capabilities = capabilities.to_vec();

                        let capabilities: Vec<xdg_toplevel::WM_CAPABILITIES> = capabilities
                            .into_iter()
                            .map(xdg_toplevel::WM_CAPABILITIES::try_from)
                            .collect::<Result<Vec<_>, _>>()
                            .unwrap();

                        println!("capabilities: {:?}", capabilities);
                    } else if header.opcode == xdg_toplevel::events::CONFIGURE {
                        let (width, args) = read_wl_u32(args);
                        let (height, args) = read_wl_u32(args);
                        let (states, _args) = read_wl_array(args);

                        println!("Received a `xdg_toplevel.configure` event.");
                        println!("Width = {width}, Height = {height}.");
                        println!("States = {:?}.", states);

                        if let ClientStatus::AwaitingInitialConfigure = self.state.status {
                            self.window_width = width;
                            self.window_height = height;

                            self.state.status = ClientStatus::GotInitialConfigure;
                        } else if width != self.window_width || height != self.window_height {
                            // Check if it is resized

                            self.window_width = width;
                            self.window_height = height;
                            self.state.status = ClientStatus::Resizing;
                        }
                    } else if header.opcode == xdg_toplevel::events::CLOSE {
                        println!("Received a `xdg_toplevel.close` event, exiting.");

                        self.state.status = ClientStatus::Closing;
                    }
                } else if (header.object_id == self.interfaces.wl_buffer_1)
                    || (header.object_id == self.interfaces.wl_buffer_2)
                        && (header.opcode == wl_buffer::events::RELEASE)
                {
                    // Buffer released
                    println!("Released wl_buffer id {}", header.object_id);

                    if let Some(surface_bufs) = &mut self.surface_buffers {
                        surface_bufs.release_buf(header.object_id);
                    }
                }

                println!();
            }

            msg_reader.finish()
        };

        self.sock.reset_buffer(remainder_offset);

        events_handled || remainder_offset > 0
    }

    fn update_state(&mut self, event_handling_in_progress: bool) -> io::Result<()> {
        match self.state.status {
            ClientStatus::Connected => {
                self.interfaces.wl_registry = self.sock.wl_display_get_registry()?;
                self.state.status = ClientStatus::GotRegistry;
            }
            ClientStatus::GotRegistry => {}
            ClientStatus::ReadingGlobalInterfaces => {
                if !event_handling_in_progress {
                    if self.state.pending_errors.is_empty() {
                        // If no more events were handled, all global interfaces have been read
                        // There were no errors either
                        self.state.status = ClientStatus::BindingInterfaces;
                    } else {
                        println!(
                            "Errors encountered during global interface registration: {:?}",
                            self.state.pending_errors
                        );
                        // TODO: Do better error handling
                        exit(1);
                    }
                }
            }
            ClientStatus::BindingInterfaces => {
                self.bind_interfaces()?;
                self.state.status = ClientStatus::AwaitingFormats;
            }
            ClientStatus::AwaitingFormats => {}
            ClientStatus::ReadingFormats => {
                if !event_handling_in_progress {
                    if self.state.pending_errors.is_empty() {
                        self.state.status = ClientStatus::CreatingSurface;
                    } else {
                        println!(
                            "Errors encountered during global interface registration: {:?}",
                            self.state.pending_errors
                        );
                        // TODO: Do better error handling
                        exit(1);
                    }
                }
            }
            ClientStatus::CreatingSurface => {
                self.create_surface()?;
                self.state.status = ClientStatus::AwaitingInitialConfigure;
            }
            ClientStatus::AwaitingInitialConfigure => {}
            ClientStatus::GotInitialConfigure => {
                if !event_handling_in_progress {
                    if self.state.pending_errors.is_empty() {
                        self.state.status = ClientStatus::CreatingBuffers;
                    } else {
                        println!(
                            "Errors encountered during global interface registration: {:?}",
                            self.state.pending_errors
                        );
                        // TODO: Do better error handling
                        exit(1);
                    }
                }
            }
            ClientStatus::CreatingBuffers => {
                self.create_buffers()?;
                self.state.status = ClientStatus::Drawing;
            }
            ClientStatus::Drawing => {
                self.draw()?;
                self.state.status = ClientStatus::Idle;
            }
            ClientStatus::Resizing => {
                if !event_handling_in_progress {
                    if self.state.pending_errors.is_empty() {
                        self.state.status = ClientStatus::Drawing;
                    } else {
                        println!(
                            "Errors encountered during global interface registration: {:?}",
                            self.state.pending_errors
                        );
                        // TODO: Do better error handling
                        exit(1);
                    }
                }
            }
            ClientStatus::Idle => {
                if let Some(serial) = self.state.pending_configure.take() {
                    self.sock
                        .xdg_surface_ack_configure(self.interfaces.xdg_surface, serial)?;
                    println!("ACKed the configure.");
                }
            }
            ClientStatus::Closing => {
                self.sock
                    .xdg_toplevel_destroy(self.interfaces.xdg_toplevel)
                    .unwrap();
                self.sock
                    .xdg_surface_destroy(self.interfaces.xdg_surface)
                    .unwrap();
                self.sock
                    .wl_surface_destroy(self.interfaces.wl_surface)
                    .unwrap();
                let wl_buffer_ids = self
                    .surface_buffers
                    .as_ref()
                    .map(|bufs| (bufs.buf_1_id, bufs.buf_2_id));

                if let Some((id_1, id_2)) = wl_buffer_ids {
                    self.sock.wl_buffer_destroy(id_1).unwrap();
                    self.sock.wl_buffer_destroy(id_2).unwrap();
                }

                self.sock
                    .wl_shm_pool_destroy(self.interfaces.wl_shm_pool)
                    .unwrap();
                self.sock.wl_shm_release(self.interfaces.wl_shm).unwrap();
                self.sock
                    .xdg_wm_base_destroy(self.interfaces.xdg_wm_base)
                    .unwrap();

                if let Some(buf) = self.surface_buffers.take() {
                    drop(buf);
                }

                self.state.status = ClientStatus::Exiting;
            }
            ClientStatus::Exiting => {
                if !event_handling_in_progress {
                    if self.state.pending_errors.is_empty() {
                        exit(0);
                    } else {
                        println!(
                            "Errors encountered during global interface registration: {:?}",
                            self.state.pending_errors
                        );
                        // TODO: Do better error handling
                        exit(1);
                    }
                }
            }
        }

        if let Some(serial) = self.state.pending_ping.take() {
            self.sock
                .xdg_wm_base_pong(self.interfaces.xdg_wm_base, serial)?;
            println!("PONGed the ping.");
        }

        Ok(())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut client = WaylandClient::new("wayland.png");

    println!("Client created");

    loop {
        client.read_events().unwrap();
        let event_handling_in_progress = client.handle_events();

        client.update_state(event_handling_in_progress).unwrap();
    }
}
