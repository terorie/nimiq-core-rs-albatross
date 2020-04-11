use algebra::mnt6_753::FqParameters;
use algebra::mnt4_753::Fr as MNT4Fr;
use crypto_primitives::prf::blake2s::constraints::blake2s_gadget_with_parameters;
use crypto_primitives::prf::Blake2sWithParameterBlock;
use r1cs_core::SynthesisError;
use r1cs_std::bits::{boolean::Boolean, uint32::UInt32};
use r1cs_std::mnt6_753::G2Gadget;
use r1cs_std::ToBitsGadget;

use crate::gadgets::{pad_point_bits, reverse_inner_byte_order, YToBitGadget};

/// This is a gadget that calculates the "state hash" in-circuit, which is simply the Blake2s
/// hash, for a given block, of the block number concatenated with the public_keys.
pub struct StateHashGadget;

impl StateHashGadget {
    /// Calculates the Blake2s hash for the block from:
    /// block number || public_keys
    pub fn evaluate<CS: r1cs_core::ConstraintSystem<MNT4Fr>>(
        mut cs: CS,
        block_number: &UInt32,
        public_keys: &Vec<G2Gadget>,
    ) -> Result<Vec<UInt32>, SynthesisError> {
        // Initialize Boolean vector.
        let mut bits: Vec<Boolean> = vec![];

        // The block number comes in little endian all the way.
        // So, a reverse will put it into big endian.
        let mut block_number_be = block_number.to_bits_le();
        block_number_be.reverse();
        bits.extend(block_number_be);

        // Convert each public key to bits and append it.
        for i in 0..public_keys.len() {
            let key = &public_keys[i];
            // Get bits from the x coordinate.
            let x_bits: Vec<Boolean> = key.x.to_bits(cs.ns(|| format!("x to bits: pk {}", i)))?;
            // Get one bit from the y coordinate.
            let greatest_bit =
                YToBitGadget::y_to_bit_g2(cs.ns(|| format!("y to bit: pk {}", i)), key)?;
            // Pad points and get *Big-Endian* representation.
            let serialized_bits = pad_point_bits::<FqParameters>(x_bits, greatest_bit);
            // Append to Boolean vector.
            bits.extend(serialized_bits);
        }

        // Prepare order of booleans for blake2s (it doesn't expect Big-Endian).
        let bits = reverse_inner_byte_order(&bits);

        // Initialize Blake2s parameters.
        let blake2s_parameters = Blake2sWithParameterBlock {
            digest_length: 32,
            key_length: 0,
            fan_out: 1,
            depth: 1,
            leaf_length: 0,
            node_offset: 0,
            xof_digest_length: 0,
            node_depth: 0,
            inner_length: 0,
            salt: [0; 8],
            personalization: [0; 8],
        };

        // Calculate hash.
        let hash = blake2s_gadget_with_parameters(
            cs.ns(|| "blake2s hash from serialized bits"),
            &bits,
            &blake2s_parameters.parameters(),
        )?;

        Ok(hash)
    }
}
