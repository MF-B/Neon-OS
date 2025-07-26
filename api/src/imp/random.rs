use crate::ptr::UserPtr;
use axerrno::LinuxResult;
use axhal::time;
use axsync::spin::SpinNoIrq;

static PARK_MILLER_LEHMER_SEED: SpinNoIrq<u32> = SpinNoIrq::new(0);
const RAND_MAX: u64 = 2_147_483_647;

pub fn random() -> u128 {
    let mut seed = PARK_MILLER_LEHMER_SEED.lock();
    if *seed == 0 {
        *seed = time::current_ticks() as u32;
    }

    let mut ret: u128 = 0;
    for _ in 0..4 {
        *seed = ((u64::from(*seed) * 48271) % RAND_MAX) as u32;
        ret = (ret << 32) | (*seed as u128);
    }
    ret
}

/// Generate random bytes and fill the buffer  
///   
/// # Arguments  
/// * `buf` - User buffer to fill with random bytes  
/// * `buflen` - Length of the buffer  
/// * `flags` - Flags (currently unused, for compatibility)  
///   
/// # Returns  
/// Number of bytes written on success  
pub fn sys_getrandom(buf: UserPtr<u8>, buflen: usize, flags: u32) -> LinuxResult<isize> {
    debug!(
        "sys_getrandom <= buf: {:?}, buflen: {}, flags: {}",
        buf.address(),
        buflen,
        flags
    );

    if buflen == 0 {
        return Ok(0);
    }

    let user_buf = buf.get_as_mut_slice(buflen)?;

    for chunk in user_buf.chunks_mut(16) {
        let random_u128 = random();
        let random_bytes = random_u128.to_le_bytes();

        let copy_len = chunk.len().min(16);
        chunk[..copy_len].copy_from_slice(&random_bytes[..copy_len]);
    }

    debug!("sys_getrandom => {}", buflen);
    Ok(buflen as isize)
}
