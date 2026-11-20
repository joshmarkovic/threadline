use std::{env, fs, path::Path};

const ICON: &str = "assets/threadline.ico";
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;
const EN_US: u16 = 0x0409;

fn main() {
    println!("cargo::rerun-if-changed={ICON}");
    let target_os = env::var("CARGO_CFG_TARGET_OS");
    let target_env = env::var("CARGO_CFG_TARGET_ENV");
    if target_os.as_deref() != Ok("windows") || target_env.as_deref() != Ok("msvc") {
        return;
    }
    // GPUI composition needs more stack headroom than the Windows linker default.
    // Reserve address space for the executable while retaining demand-committed pages.
    println!("cargo::rustc-link-arg-bin=threadline=/STACK:8388608");

    // GPUI loads icon group 1 from the executable for the window, taskbar and Alt+Tab.
    // The linker accepts a compiled resource file, so no resource compiler is needed.
    let icon = fs::read(ICON).expect("the application icon is readable");
    let version = env::var("CARGO_PKG_VERSION").expect("Cargo sets the package version");
    let mut resources = resource(0, 0, 0, &[]);
    resources.extend(icon_resources(&icon));
    resources.extend(resource(RT_VERSION, 1, 0x0030, &version_info(&version)));
    let out_dir = env::var("OUT_DIR").expect("Cargo sets OUT_DIR");
    let path = Path::new(&out_dir).join("threadline.res");
    fs::write(&path, resources).expect("the resource file is writable");
    println!("cargo::rustc-link-arg-bin=threadline={}", path.display());
}

fn pad(bytes: &mut Vec<u8>) {
    bytes.resize(bytes.len().next_multiple_of(4), 0);
}

/// One entry of a compiled resource file with a numeric type and name.
fn resource(kind: u16, name: u16, memory_flags: u16, data: &[u8]) -> Vec<u8> {
    let mut entry = Vec::with_capacity(32 + data.len() + 3);
    entry.extend((data.len() as u32).to_le_bytes());
    entry.extend(32u32.to_le_bytes());
    for id in [kind, name] {
        entry.extend(0xffffu16.to_le_bytes());
        entry.extend(id.to_le_bytes());
    }
    entry.extend(0u32.to_le_bytes());
    entry.extend(memory_flags.to_le_bytes());
    entry.extend(if kind == 0 { 0 } else { EN_US }.to_le_bytes());
    entry.extend([0; 8]);
    entry.extend(data);
    pad(&mut entry);
    entry
}

/// Splits an icon file into one resource per image plus the group that indexes them.
fn icon_resources(icon: &[u8]) -> Vec<u8> {
    let word = |at: usize| u16::from_le_bytes([icon[at], icon[at + 1]]);
    let dword = |at: usize| u32::from_le_bytes(icon[at..at + 4].try_into().expect("four bytes"));
    assert!(
        icon.len() >= 6 && word(0) == 0 && word(2) == 1,
        "{ICON} is not an icon file"
    );
    let count = word(4);
    let mut images = Vec::new();
    let mut group = Vec::new();
    group.extend([0, 0, 1, 0]);
    group.extend(count.to_le_bytes());
    for index in 0..count {
        let entry = 6 + 16 * usize::from(index);
        let (size, offset) = (dword(entry + 8) as usize, dword(entry + 12) as usize);
        let id = index + 1;
        images.extend(resource(RT_ICON, id, 0x1010, &icon[offset..offset + size]));
        // A group entry repeats the file entry and names the image instead of its offset.
        group.extend(&icon[entry..entry + 12]);
        group.extend(id.to_le_bytes());
    }
    images.extend(resource(RT_GROUP_ICON, 1, 0x1030, &group));
    images
}

fn wide(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// One block of a version resource: a length header, a key, a value and child blocks.
fn block(key: &str, value: &[u8], value_len: u16, text: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0; 6];
    out.extend(wide(key));
    pad(&mut out);
    out.extend(value);
    for child in children {
        pad(&mut out);
        out.extend(child);
    }
    let len = out.len() as u16;
    out[..2].copy_from_slice(&len.to_le_bytes());
    out[2..4].copy_from_slice(&value_len.to_le_bytes());
    out[4..6].copy_from_slice(&u16::from(text).to_le_bytes());
    out
}

/// Names the program in Task Manager and the file's Properties dialog.
fn version_info(version: &str) -> Vec<u8> {
    let mut parts = version
        .split(['.', '-', '+'])
        .map(|part| part.parse::<u16>().unwrap_or(0));
    let mut next = || u32::from(parts.next().unwrap_or(0));
    let (major, minor, patch) = (next(), next(), next());
    let (most, least) = (major << 16 | minor, patch << 16);
    let fixed: Vec<u8> = [
        0xfeef_04bd,
        0x0001_0000,
        most,
        least,
        most,
        least,
        0x3f,
        0,
        0x0004_0004,
        1,
        0,
        0,
        0,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    let strings: Vec<Vec<u8>> = [
        ("CompanyName", "Threadline"),
        ("FileDescription", "Threadline"),
        ("FileVersion", version),
        ("InternalName", "threadline"),
        ("OriginalFilename", "threadline.exe"),
        ("ProductName", "Threadline"),
        ("ProductVersion", version),
    ]
    .into_iter()
    .map(|(key, value)| {
        let value = wide(value);
        block(key, &value, (value.len() / 2) as u16, true, &[])
    })
    .collect();
    let table = block("040904B0", &[], 0, true, &strings);
    let translation = block("Translation", &[0x09, 0x04, 0xb0, 0x04], 4, false, &[]);
    block(
        "VS_VERSION_INFO",
        &fixed,
        fixed.len() as u16,
        false,
        &[
            block("StringFileInfo", &[], 0, true, &[table]),
            block("VarFileInfo", &[], 0, true, &[translation]),
        ],
    )
}
