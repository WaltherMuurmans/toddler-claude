//! Per-user encrypted token storage using Windows DPAPI (CryptProtectData),
//! persisted as hex-encoded ciphertext files in %APPDATA%\ToddlerClaude\secrets\.
//! DPAPI ties the ciphertext to the current Windows user login, so other users
//! on the machine cannot decrypt it.

use anyhow::{anyhow, Result};
use std::path::PathBuf;

pub mod keys {
    pub const CLAUDE_TOKEN: &str = "claude_oauth_token";
    pub const GITHUB_TOKEN: &str = "github_pat";
    pub const FLY_TOKEN: &str = "fly_api_token";
}

fn dir() -> Result<PathBuf> {
    let base = dirs::config_dir()
        .or_else(dirs::data_local_dir)
        .ok_or_else(|| anyhow!("no config dir"))?;
    let d = base.join("ToddlerClaude").join("secrets");
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

fn path_for(key: &str) -> Result<PathBuf> {
    Ok(dir()?.join(format!("{key}.bin")))
}

#[cfg(windows)]
fn protect(plain: &[u8]) -> Result<Vec<u8>> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CRYPT_INTEGER_BLOB,
    };
    use windows::Win32::Foundation::HLOCAL;

    let data_in = CRYPT_INTEGER_BLOB {
        cbData: plain.len() as u32,
        pbData: plain.as_ptr() as *mut u8,
    };
    let mut data_out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &data_in,
            PWSTR::null(),
            None,
            None,
            None,
            0,
            &mut data_out,
        )
        .map_err(|e| anyhow!("CryptProtectData: {e}"))?;
    }
    let slice =
        unsafe { std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize) };
    let v = slice.to_vec();
    unsafe {
        let _ = LocalFree(HLOCAL(data_out.pbData as *mut _));
    }
    Ok(v)
}

#[cfg(windows)]
fn unprotect(cipher: &[u8]) -> Result<Vec<u8>> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPT_INTEGER_BLOB,
    };

    let data_in = CRYPT_INTEGER_BLOB {
        cbData: cipher.len() as u32,
        pbData: cipher.as_ptr() as *mut u8,
    };
    let mut data_out = CRYPT_INTEGER_BLOB::default();
    let mut pwsz: PWSTR = PWSTR::null();
    unsafe {
        CryptUnprotectData(
            &data_in,
            Some(&mut pwsz),
            None,
            None,
            None,
            0,
            &mut data_out,
        )
        .map_err(|e| anyhow!("CryptUnprotectData: {e}"))?;
    }
    let slice =
        unsafe { std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize) };
    let v = slice.to_vec();
    unsafe {
        if !pwsz.is_null() {
            let _ = LocalFree(HLOCAL(pwsz.0 as *mut _));
        }
        let _ = LocalFree(HLOCAL(data_out.pbData as *mut _));
    }
    Ok(v)
}

#[cfg(not(windows))]
fn protect(plain: &[u8]) -> Result<Vec<u8>> {
    Ok(plain.to_vec())
}

#[cfg(not(windows))]
fn unprotect(cipher: &[u8]) -> Result<Vec<u8>> {
    Ok(cipher.to_vec())
}

pub fn set(key: &str, value: &str) -> Result<()> {
    let path = path_for(key)?;
    let blob = protect(value.as_bytes())?;
    std::fs::write(&path, &blob)?;
    Ok(())
}

pub fn get(key: &str) -> Result<Option<String>> {
    let path = path_for(key)?;
    if !path.exists() {
        return Ok(None);
    }
    let blob = std::fs::read(&path)?;
    let plain = unprotect(&blob)?;
    Ok(Some(String::from_utf8(plain)?))
}

pub fn delete(key: &str) -> Result<()> {
    let path = path_for(key)?;
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}
