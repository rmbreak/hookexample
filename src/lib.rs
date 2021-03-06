use std::ops::{Deref, DerefMut};
use std::sync::Mutex;

use argh::FromArgs;
use lazy_static::lazy_static;
use log::info;
use widestring::{U16CStr, U16CString};
use winapi::ctypes::{c_char, wchar_t};
use winapi::shared::minwindef::*;
use winapi::um::winnt::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};

use hook::*;

mod hook;

#[derive(FromArgs, Debug)]
/// Nox in-game Hax CLI
struct HaxCLI {
    #[argh(subcommand)]
    subcommand: HaxSubcommandEnum,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand)]
enum HaxSubcommandEnum {
    Entity(EntitySubcommand),
    Enable(EnableSubcommand),
}

#[derive(FromArgs, Debug)]
/// Map entities
#[argh(subcommand, name = "entities")]
struct EntitySubcommand {
    /// dump all entities
    #[argh(switch, short = 'd')]
    dump: bool,
}

#[derive(FromArgs, Debug)]
/// Map entities
#[argh(subcommand, name = "enable")]
struct EnableSubcommand {
    /// enable circle mode
    #[argh(switch, short = 'c')]
    circle: bool,
}

#[no_mangle]
#[allow(non_snake_case, unused_variables)]
pub extern "system" fn DllMain(
    dll_module: HINSTANCE,
    call_reason: DWORD,
    reserved: LPVOID,
) -> BOOL {
    match call_reason {
        DLL_PROCESS_ATTACH => on_dll_process_attach(),
        DLL_PROCESS_DETACH => on_dll_process_detach(),
        _ => (),
    }

    return TRUE;
}

fn on_dll_process_attach() {
    // Give us a console window to write to
    unsafe { winapi::um::consoleapi::AllocConsole() };

    // Create a simple logger so we can use debug, info, error and friends
    simple_logger::init().unwrap();

    info!("Setting up hooks...");

    // Enable hooks
    enable_hook!(DETOUR_CONSOLE_WRITE);
    enable_hook!(DETOUR_SPAWN_ITEM);
}

fn on_dll_process_detach() {
    info!("Tearing down hooks...");

    // Disable hooks
    disable_hook!(DETOUR_CONSOLE_WRITE);
    disable_hook!(DETOUR_SPAWN_ITEM);

    // Detach the console
    unsafe { winapi::um::wincon::FreeConsole() };
}

#[repr(C)]
struct Entity {
    // +0x00
    unk1: [u8; 0x28],
    // +0x28
    extent: u16,
    // +0x2a
    unk2: [u8; 0x16],
    // +0x40
    xcoord: f32,
    // +0x44
    ycoord: f32,
    // +0x48
    unk3: [u8; 0x194],
    // +0x1dc
    next_entity: *mut Entity,
}

impl Entity {
    const START_ENTITY_ADDRESS: *const *mut Entity = 0x00750708 as *const *mut Entity;

    fn player() -> &'static mut Entity {
        let mut entity: &mut Entity = unsafe { std::mem::transmute(*Self::START_ENTITY_ADDRESS) };
        while entity.next_entity != 0 as _ && entity.extent != 0 {
            entity = unsafe { std::mem::transmute(entity.next_entity) };
        }

        entity
    }

    fn dump_map_entities() {
        let mut entity: &mut Entity = unsafe { std::mem::transmute(*Self::START_ENTITY_ADDRESS) };
        info!(
            "entity = &entity: {:?}, extent: {}, xcoord: {}, ycoord: {}",
            entity as *const _, entity.extent, entity.xcoord, entity.ycoord
        );
        while entity.next_entity != 0 as _ {
            entity = unsafe { std::mem::transmute(entity.next_entity) };
            info!(
                "entity = &entity: {:?}, extent: {}, xcoord: {}, ycoord: {}",
                entity as *const _, entity.extent, entity.xcoord, entity.ycoord
            );
            if entity.extent == 0 {
                entity.xcoord += 50.0;
            }
        }
    }
}

// Function signatures
type ConsoleWriteFn = extern "cdecl" fn(TextColor, *const wchar_t) -> BOOL;
type SpawnItemFn = extern "cdecl" fn(*const c_char) -> u32;

// Hook setup
setup_hook!(
    DETOUR_CONSOLE_WRITE,
    ConsoleWriteFn,
    0x00450b90,
    detour_console_write
);
setup_hook!(
    DETOUR_SPAWN_ITEM,
    SpawnItemFn,
    0x004e3810,
    detour_spawn_item
);

lazy_static! {
    static ref LAST_MESSAGE_HAX: Mutex<bool> = Mutex::new(false);
}

#[derive(Debug)]
#[repr(u32)]
pub enum TextColor {
    Black = 1,
    Grey = 2,
    White = 3,
    White2 = 4,
    DarkRed = 5,
    Red = 6,
    LightRed = 7,
    DarkGreen = 8,
    Green = 9,
    LightGreen = 10,
    DarkBlue = 11,
    Blue = 12,
    LightBlue = 13,
    DarkYellow = 14,
    Yellow = 15,
    LightYellow = 16,
}

extern "cdecl" fn detour_console_write(color: TextColor, message: *const wchar_t) -> BOOL {
    info!(
        "detour_console_write(color={:?}, message={:?})",
        color, message
    );

    // Convert message to a utf8 string
    let s = unsafe { U16CStr::from_ptr_str(message).to_string_lossy() };

    if *LAST_MESSAGE_HAX.lock().unwrap() {
        *LAST_MESSAGE_HAX.lock().unwrap() = false;
        return TRUE;
    }

    if s.starts_with("> /hax") {
        *LAST_MESSAGE_HAX.lock().unwrap() = true;
        DETOUR_CONSOLE_WRITE
            .lock()
            .unwrap()
            .call(TextColor::LightBlue, message);

        let args = &s[6..].split_ascii_whitespace().collect::<Vec<_>>();
        match HaxCLI::from_args(&["hax"], &args) {
            Ok(cmd) => {
                DETOUR_CONSOLE_WRITE.lock().unwrap().call(
                    TextColor::White,
                    U16CString::from_str(format!("{:?}", cmd)).unwrap().as_ptr(),
                );

                match cmd.subcommand {
                    HaxSubcommandEnum::Entity(cmd) => {
                        if cmd.dump {
                            Entity::dump_map_entities();
                        }
                    }

                    HaxSubcommandEnum::Enable(cmd) => {
                        if cmd.circle {
                            circle();
                        }
                    }
                }
            }

            Err(exit) => {
                DETOUR_CONSOLE_WRITE.lock().unwrap().call(
                    TextColor::LightRed,
                    U16CString::from_str(&exit.output).unwrap().as_ptr(),
                );
            }
        }

        return TRUE;
    }
    return DETOUR_CONSOLE_WRITE.lock().unwrap().call(color, message);
}

#[derive(Debug)]
struct PlayerCircle {
    /// x, y
    origin: (f32, f32),
    radius: f32,
}

struct UnsafeEntity(&'static mut Entity);

unsafe impl Send for UnsafeEntity {}

impl Deref for UnsafeEntity {
    type Target = Entity;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for UnsafeEntity {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

fn circle() {
    let mut player = UnsafeEntity(Entity::player());
    let pc = PlayerCircle {
        origin: (player.xcoord, player.ycoord),
        radius: 100.0,
    };

    std::thread::spawn(move || {
        for i in 0.. {
            let x = pc.origin.0 + pc.radius * ((i % 360) as f32 * 0.01745).cos();
            let y = pc.origin.1 + pc.radius * ((i % 360) as f32 * 0.01745).sin();

            player.xcoord = x;
            player.ycoord = y;

            std::thread::sleep(std::time::Duration::from_millis(3000 / 360));
        }
    });
}

extern "cdecl" fn detour_spawn_item(itemname: *const c_char) -> u32 {
    let item = unsafe { std::ffi::CString::from_raw(std::mem::transmute(itemname)) };
    info!("detour_spawn_item(itemname=\"{}\")", item.to_string_lossy());
    std::mem::forget(item);

    return DETOUR_SPAWN_ITEM.lock().unwrap().call(itemname);
}
