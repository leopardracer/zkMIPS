#![no_std]
#![no_main]

use zkm_zkvm::syscalls::syscall_secp256r1_decompress;

zkm_zkvm::entrypoint!(main);

pub fn main() {
    let compressed_key: [u8; 33] = zkm_zkvm::io::read_vec().try_into().unwrap();

    for _ in 0..4 {
        let mut decompressed_key: [u8; 64] = [0; 64];
        decompressed_key[..32].copy_from_slice(&compressed_key[1..]);
        let is_odd = match compressed_key[0] {
            2 => false,
            3 => true,
            _ => panic!("Invalid compressed key {:X?}",  compressed_key),
        };
        syscall_secp256r1_decompress(&mut decompressed_key, is_odd);

        let mut result: [u8; 65] = [0; 65];
        result[0] = 4;
        result[1..].copy_from_slice(&decompressed_key);

        zkm_zkvm::io::commit_slice(&result);
    }
}
