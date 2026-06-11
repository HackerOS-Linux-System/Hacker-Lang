use anyhow::{bail, Context, Result};
use crate::bytecode::{HlBcHeader, HlModule};
use std::path::Path;

pub const BC_MAGIC: &[u8; 4] = b"HLBC";
pub const BC_VERSION: u32 = 2;

/// Shebang dla pliku .bc — `hl run` uruchamia bytecode przez JIT
const BC_SHEBANG: &str = "#!/usr/bin/env -S /usr/bin/hl run\n";

pub fn write_bc_file(module: &HlModule, path: &Path) -> Result<()> {

    let mut buf: Vec<u8> = Vec::with_capacity(4096);

    // Shebang (musi być pierwszy żeby plik był wykonywalny bezpośrednio)
    buf.extend_from_slice(BC_SHEBANG.as_bytes());

    // Magic
    buf.extend_from_slice(BC_MAGIC);

    // Wersja (4 bajty LE)
    buf.extend_from_slice(&BC_VERSION.to_le_bytes());

    // JSON header
    let header_json = serde_json::to_vec(&module.header)
    .context("Serializacja nagłówka .bc")?;
    let header_len = header_json.len() as u64;
    buf.extend_from_slice(&header_len.to_le_bytes());
    buf.extend_from_slice(&header_json);

    // Moduł (bincode) — szybszy i mniejszy niż JSON
    let module_bytes = bincode::serialize(module)
    .context("Serializacja modułu .bc")?;
    buf.extend_from_slice(&module_bytes);

    // Zapisz
    std::fs::write(path, &buf).with_context(|| format!("Zapis .bc: {:?}", path))?;

    // Ustaw bit wykonywalny
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }

    tracing::debug!("Zapisano .bc ({} bajtów): {:?}", buf.len(), path);
    Ok(())
}

pub fn read_bc_file(path: &Path) -> Result<HlModule> {
    let raw = std::fs::read(path)
    .with_context(|| format!("Odczyt .bc: {:?}", path))?;

    parse_bc_bytes(&raw, path)
}

pub fn parse_bc_bytes(raw: &[u8], path: &Path) -> Result<HlModule> {
    let mut pos = 0usize;

    // Pomiń shebang jeśli jest
    if raw.starts_with(b"#!") {
        if let Some(nl) = raw.iter().position(|&b| b == b'\n') {
            pos = nl + 1;
        }
    }

    // Magic
    if raw.len() < pos + 4 {
        bail!("Plik .bc za krótki: {:?}", path);
    }
    if &raw[pos..pos+4] != BC_MAGIC {
        bail!("Nieprawidłowy magic w .bc: {:?} (czy to plik .bc?)", path);
    }
    pos += 4;

    // Wersja
    if raw.len() < pos + 4 {
        bail!("Urwany nagłówek .bc: {:?}", path);
    }
    let ver = u32::from_le_bytes(raw[pos..pos+4].try_into().unwrap());
    if ver != BC_VERSION {
        bail!("Niezgodna wersja .bc: {} (oczekiwano {}): {:?}", ver, BC_VERSION, path);
    }
    pos += 4;

    // JSON header length
    if raw.len() < pos + 8 {
        bail!("Urwany nagłówek JSON w .bc: {:?}", path);
    }
    let header_len = u64::from_le_bytes(raw[pos..pos+8].try_into().unwrap()) as usize;
    pos += 8;

    // JSON header (tylko do walidacji / logowania)
    if raw.len() < pos + header_len {
        bail!("Urwane dane nagłówka JSON w .bc: {:?}", path);
    }
    let _header: HlBcHeader = serde_json::from_slice(&raw[pos..pos+header_len])
    .context("Parsowanie nagłówka .bc")?;
    pos += header_len;

    // Bincode module
    let module: HlModule = bincode::deserialize(&raw[pos..])
    .context("Deserializacja modułu .bc")?;

    Ok(module)
}

/// Sprawdź czy plik to poprawny .bc (szybkie sprawdzenie bez pełnego parsowania)
pub fn is_bc_file(path: &Path) -> bool {
    let Ok(f) = std::fs::File::open(path) else { return false; };
    use std::io::Read;
    let mut buf = [0u8; 64];
    let Ok(n) = std::io::BufReader::new(f).read(&mut buf) else { return false; };
    let data = &buf[..n];

    // Pomiń shebang
    let start = if data.starts_with(b"#!") {
        data.iter().position(|&b| b == b'\n').map(|i| i+1).unwrap_or(0)
    } else { 0 };

    data.get(start..start+4) == Some(BC_MAGIC)
}
