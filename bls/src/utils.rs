use algebra::BigInteger768;

/// Transforms a vector of bytes into the corresponding vector of bits (booleans).
pub fn bytes_to_bits(bytes: &[u8]) -> Vec<bool> {
    let mut bits = vec![];

    for i in 0..bytes.len() {
        let byte = bytes[i];
        for j in (0..8).rev() {
            bits.push((byte >> j) & 1 == 1);
        }
    }

    bits
}

/// Creates a BigInteger from an array of bytes in big-endian format.
pub fn big_int_from_bytes_be<R: std::io::Read>(reader: &mut R) -> BigInteger768 {
    let mut res = [0u64; 12];

    for num in res.iter_mut().rev() {
        let mut bytes = [0u8; 8];
        reader.read_exact(&mut bytes).unwrap();
        *num = u64::from_be_bytes(bytes);
    }

    BigInteger768::new(res)
}
