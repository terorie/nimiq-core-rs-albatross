use std::borrow::{Borrow, Cow};

use algebra::bls12_377::{Fq, FqParameters};
use algebra::{sw6::Fr as SW6Fr, One};
use crypto_primitives::{prf::blake2s::constraints::Blake2sOutputGadget, FixedLengthCRHGadget};
use r1cs_core::{ConstraintSystem, SynthesisError};
use r1cs_std::bits::{boolean::Boolean, uint32::UInt32, uint8::UInt8};
use r1cs_std::bls12_377::{FqGadget, G1Gadget, G2Gadget};
use r1cs_std::prelude::{AllocGadget, CondSelectGadget, FieldGadget, GroupGadget};
use r1cs_std::{Assignment, ToBitsGadget};

use crate::constants::VALIDATOR_SLOTS;
use crate::gadgets::{
    pad_point_bits, reverse_inner_byte_order, CRHGadget, CRHGadgetParameters, CheckSigGadget,
    SmallerThanGadget, YToBitGadget,
};
use crate::primitives::MacroBlock;

/// A simple enum representing the two rounds of signing in the macro blocks.
#[derive(Clone, Copy, Ord, PartialOrd, PartialEq, Eq)]
pub enum Round {
    Prepare = 0,
    Commit = 1,
}

/// A gadget representing a macro block in Albatross.
pub struct MacroBlockGadget {
    pub header_hash: Vec<Boolean>,
    pub public_keys: Vec<G2Gadget>,
    pub prepare_signature: G1Gadget,
    pub prepare_signer_bitmap: Vec<Boolean>,
    pub commit_signature: G1Gadget,
    pub commit_signer_bitmap: Vec<Boolean>,
}

impl MacroBlockGadget {
    /// A convenience method to return the public keys of the macro block.
    pub fn public_keys(&self) -> &[G2Gadget] {
        &self.public_keys
    }

    /// A function that verifies the validity of a given macro block. It is the main function for
    /// the macro block gadget.
    pub fn verify<CS: ConstraintSystem<SW6Fr>>(
        &self,
        mut cs: CS,
        // This is the set of public keys that signed this macro block. Corresponds to the previous
        // set of validators.
        prev_public_keys: &[G2Gadget],
        // This is the maximum number of non-signers for the block. It is exclusive, meaning that if
        // number of non-signers == max_non-signers then the block is NOT valid.
        // Note: Some confusion might arise because the constant MAX_NON_SIGNERS in constants.rs is
        // an inclusive maximum. These two values are different,
        // (max_non_signers here) == (MAX_NON_SIGNERS in constants.rs) + 1
        max_non_signers: &FqGadget,
        // Simply the number of the macro block.
        block_number: &UInt32,
        // The generator used in the BLS signature scheme. It is the generator used to create public
        // keys.
        sig_generator: &G2Gadget,
        // The two next generators are only needed because the elliptic curve addition in-circuit is
        // incomplete. Meaning that it can't handle the identity element (aka zero, aka point-at-infinity).
        // So, these generators are needed to do running sums. Instead of starting at zero, we start
        // with the generator and subtract it at the end of the running sum.
        sum_generator_g1: &G1Gadget,
        sum_generator_g2: &G2Gadget,
        // These are just the parameters for the Pedersen hash gadget.
        crh_parameters: &CRHGadgetParameters,
    ) -> Result<(), SynthesisError> {
        // Get the hash point and the aggregated public key for the prepare round of signing.
        let (hash0, pub_key0) = self.get_hash_and_public_keys(
            cs.ns(|| "prepare"),
            Round::Prepare,
            prev_public_keys,
            max_non_signers,
            block_number,
            sum_generator_g2,
            crh_parameters,
        )?;

        // Get the hash point and the aggregated public key for the commit round of signing.
        let (hash1, pub_key1) = self.get_hash_and_public_keys(
            cs.ns(|| "commit"),
            Round::Commit,
            prev_public_keys,
            max_non_signers,
            block_number,
            sum_generator_g2,
            crh_parameters,
        )?;

        // Add together the two aggregated signatures for the prepare and commit rounds of signing.
        // Note the use of the generator to avoid an error in the sum.
        let mut signature =
            sum_generator_g1.add(cs.ns(|| "add prepare sig"), &self.prepare_signature)?;
        signature = signature.add(cs.ns(|| "add commit sig"), &self.commit_signature)?;
        signature = signature.sub(cs.ns(|| "finalize sig"), sum_generator_g1)?;

        // Verifies the validity of the signatures.
        CheckSigGadget::check_signatures(
            cs.ns(|| "check signatures"),
            &[pub_key0, pub_key1],
            sig_generator,
            &signature,
            &[hash0, hash1],
        )?;

        Ok(())
    }

    /// A function that returns the aggregated public key and the hash point, for a given round,
    /// of the macro block.
    fn get_hash_and_public_keys<CS: ConstraintSystem<SW6Fr>>(
        &self,
        mut cs: CS,
        round: Round,
        prev_public_keys: &[G2Gadget],
        max_non_signers: &FqGadget,
        block_number: &UInt32,
        generator: &G2Gadget,
        crh_parameters: &CRHGadgetParameters,
    ) -> Result<(G1Gadget, G2Gadget), SynthesisError> {
        // Calculate the Pedersen hash for the given macro block and round.
        let hash_point = self.hash(
            cs.ns(|| "create hash point"),
            round,
            block_number,
            crh_parameters,
        )?;

        // Choose signer bitmap and signature based on round.
        let signer_bitmap = match round {
            Round::Prepare => &self.prepare_signer_bitmap,
            Round::Commit => &self.commit_signer_bitmap,
        };

        // Also, during the commit round, we need to check the max-non-signer restriction
        // with respect to prepare_signer_bitmap & commit_signer_bitmap.
        let reference_bitmap = match round {
            Round::Prepare => None,
            Round::Commit => Some(self.prepare_signer_bitmap.as_ref()),
        };

        // Calculate the an aggregate public key given the set of public keys and a bitmap of signers.
        let aggregate_public_key = Self::aggregate_public_key(
            cs.ns(|| "aggregate public keys"),
            prev_public_keys,
            signer_bitmap,
            reference_bitmap,
            max_non_signers,
            generator,
        )?;

        Ok((hash_point, aggregate_public_key))
    }

    /// A function that calculates the Pedersen hash for the block from:
    /// round number || block number || header_hash || public_keys
    /// where || means concatenation.
    /// Note that the Pedersen hash is only collision-resistant
    /// and does not provide pseudo-random output! Such pseudo-randomness is necessary for
    /// the BLS signature scheme.
    /// For our use-case, however, this suffices as the header_hash field
    /// already provides sufficient entropy.
    pub fn hash<CS: r1cs_core::ConstraintSystem<SW6Fr>>(
        &self,
        mut cs: CS,
        round: Round,
        block_number: &UInt32,
        crh_parameters: &CRHGadgetParameters,
    ) -> Result<G1Gadget, SynthesisError> {
        // Initialize Boolean vector.
        let mut bits: Vec<Boolean> = vec![];

        // The round number comes in little endian,
        // which is why we need to reverse the bits to get big endian.
        let round_number = UInt8::constant(round as u8);
        let mut round_number_bits = round_number.into_bits_le();
        round_number_bits.reverse();
        bits.append(&mut round_number_bits);

        // The block number comes in little endian all the way.
        // So, a reverse will put it into big endian.
        let mut block_number_be = block_number.to_bits_le();
        block_number_be.reverse();
        bits.append(&mut block_number_be);

        // Append the header hash.
        bits.extend_from_slice(&self.header_hash);

        // Serialize all the public keys.
        for i in 0..self.public_keys.len() {
            let key = &self.public_keys[i];
            // Get bits from the x coordinate.
            let x_bits: Vec<Boolean> = key.x.to_bits(cs.ns(|| format!("x to bits: pk {}", i)))?;
            // Get one bit from the y coordinate.
            let greatest_bit =
                YToBitGadget::y_to_bit_g2(cs.ns(|| format!("y to bits: pk {}", i)), key)?;
            // Pad points and get *Big-Endian* representation.
            let serialized_bits = pad_point_bits::<FqParameters>(x_bits, greatest_bit);
            // Append to Boolean vector.
            bits.extend(serialized_bits);
        }

        // Prepare order of booleans for Pedersen hash.
        let bits = reverse_inner_byte_order(&bits);
        let input_bytes: Vec<UInt8> = bits
            .chunks(8)
            .map(|chunk| UInt8::from_bits_le(chunk))
            .collect();

        // Finally feed the serialized bits into the Pedersen hash gadget.
        let crh_result = CRHGadget::check_evaluation_gadget(
            &mut cs.ns(|| "crh_evaluation"),
            crh_parameters,
            &input_bytes,
        )?;

        Ok(crh_result)
    }

    /// A function that aggregates the public keys of all the validators that signed the block. If
    /// it is performing the aggregation for the commit round, then it will also check if every signer
    /// in the commit round was also a signer in the prepare round.
    pub fn aggregate_public_key<CS: r1cs_core::ConstraintSystem<SW6Fr>>(
        mut cs: CS,
        public_keys: &[G2Gadget],
        key_bitmap: &[Boolean],
        reference_bitmap: Option<&[Boolean]>,
        max_non_signers: &FqGadget,
        generator: &G2Gadget,
    ) -> Result<G2Gadget, SynthesisError> {
        // Initialize the running sums.
        // Note that we initialize the public key sum to the generator, not to zero.
        let mut num_non_signers = FqGadget::zero(cs.ns(|| "number used public keys"))?;
        let mut sum = Cow::Borrowed(generator);

        // Conditionally add all other public keys.
        for (i, (key, included)) in public_keys.iter().zip(key_bitmap.iter()).enumerate() {
            // Calculate a new sum that includes the next public key.
            let new_sum = sum.add(cs.ns(|| format!("add public key {}", i)), key)?;

            // Choose either the new public key sum or the old public key sum, depending on whether
            // the bitmap indicates that the validator signed or not.
            let cond_sum = CondSelectGadget::conditionally_select(
                cs.ns(|| format!("conditionally add public key {}", i)),
                included,
                &new_sum,
                sum.as_ref(),
            )?;

            // If there is a reference bitmap, we only count such signatures as included
            // that fulfill included & reference[i]. That means, we only count these that
            // also signed in the reference bitmap (usually the prepare phase).
            let included = if let Some(reference) = reference_bitmap {
                Boolean::and(
                    cs.ns(|| format!("included & reference[{}]", i)),
                    included,
                    &reference[i],
                )?
            } else {
                *included
            };

            // Update the number of non-signers. Note that the bitmap is negated to get the
            // non-signers: ~(included).
            num_non_signers = num_non_signers.conditionally_add_constant(
                cs.ns(|| format!("public key count {}", i)),
                &included.not(),
                Fq::one(),
            )?;

            // Update the public key sum.
            sum = Cow::Owned(cond_sum);
        }

        // Finally subtract the generator from the sum to get the correct value.
        sum = Cow::Owned(sum.sub(cs.ns(|| "finalize aggregate public key"), generator)?);

        // Enforce that there are enough signers.
        // Note that we don't verify equality.
        SmallerThanGadget::enforce_smaller_than(
            cs.ns(|| "enforce non signers"),
            &num_non_signers,
            max_non_signers,
        )?;

        Ok(sum.into_owned())
    }
}

/// The allocation function for the macro block gadget.
impl AllocGadget<MacroBlock, SW6Fr> for MacroBlockGadget {
    /// This is the allocation function for a private input.
    fn alloc<F, T, CS: ConstraintSystem<SW6Fr>>(
        mut cs: CS,
        value_gen: F,
    ) -> Result<Self, SynthesisError>
    where
        F: FnOnce() -> Result<T, SynthesisError>,
        T: Borrow<MacroBlock>,
    {
        let empty_block = MacroBlock::default();

        let value = match value_gen() {
            Ok(val) => val.borrow().clone(),
            Err(_) => empty_block,
        };

        assert_eq!(value.public_keys.len(), VALIDATOR_SLOTS);

        let header_hash =
            Blake2sOutputGadget::alloc(cs.ns(|| "header hash"), || Ok(&value.header_hash))?;

        // While the bytes of the Blake2sOutputGadget start with the most significant first,
        // the bits internally start with the least significant.
        // Thus, we need to reverse the bit order there.
        let header_hash = header_hash
            .0
            .into_iter()
            .flat_map(|n| reverse_inner_byte_order(&n.into_bits_le()))
            .collect::<Vec<Boolean>>();

        let public_keys =
            Vec::<G2Gadget>::alloc(cs.ns(|| "public keys"), || Ok(&value.public_keys[..]))?;

        let prepare_signer_bitmap =
            Vec::<Boolean>::alloc(cs.ns(|| "prepare signer bitmap"), || {
                Ok(&value.prepare_signer_bitmap[..])
            })?;

        let prepare_signature = G1Gadget::alloc(cs.ns(|| "prepare signature"), || {
            value.prepare_signature.get()
        })?;

        let commit_signer_bitmap = Vec::<Boolean>::alloc(cs.ns(|| "commit signer bitmap"), || {
            Ok(&value.commit_signer_bitmap[..])
        })?;

        let commit_signature = G1Gadget::alloc(cs.ns(|| "commit signature"), || {
            value.commit_signature.get()
        })?;

        Ok(MacroBlockGadget {
            header_hash,
            public_keys,
            prepare_signature,
            prepare_signer_bitmap,
            commit_signature,
            commit_signer_bitmap,
        })
    }

    /// This is the allocation function for a private input.
    fn alloc_input<F, T, CS: ConstraintSystem<SW6Fr>>(
        mut cs: CS,
        value_gen: F,
    ) -> Result<Self, SynthesisError>
    where
        F: FnOnce() -> Result<T, SynthesisError>,
        T: Borrow<MacroBlock>,
    {
        let empty_block = MacroBlock::default();

        let value = match value_gen() {
            Ok(val) => val.borrow().clone(),
            Err(_) => empty_block,
        };

        assert_eq!(value.public_keys.len(), VALIDATOR_SLOTS);

        let header_hash =
            Blake2sOutputGadget::alloc_input(cs.ns(|| "header hash"), || Ok(&value.header_hash))?;

        // While the bytes of the Blake2sOutputGadget start with the most significant first,
        // the bits internally start with the least significant.
        // Thus, we need to reverse the bit order there.
        let header_hash = header_hash
            .0
            .into_iter()
            .flat_map(|n| reverse_inner_byte_order(&n.into_bits_le()))
            .collect::<Vec<Boolean>>();

        let public_keys =
            Vec::<G2Gadget>::alloc_input(cs.ns(|| "public keys"), || Ok(&value.public_keys[..]))?;

        let prepare_signer_bitmap =
            Vec::<Boolean>::alloc_input(cs.ns(|| "prepare signer bitmap"), || {
                Ok(&value.prepare_signer_bitmap[..])
            })?;

        let prepare_signature = G1Gadget::alloc_input(cs.ns(|| "prepare signature"), || {
            value.prepare_signature.get()
        })?;

        let commit_signer_bitmap =
            Vec::<Boolean>::alloc_input(cs.ns(|| "commit signer bitmap"), || {
                Ok(&value.commit_signer_bitmap[..])
            })?;

        let commit_signature = G1Gadget::alloc_input(cs.ns(|| "commit signature"), || {
            value.commit_signature.get()
        })?;

        Ok(MacroBlockGadget {
            header_hash,
            public_keys,
            prepare_signature,
            prepare_signer_bitmap,
            commit_signature,
            commit_signer_bitmap,
        })
    }
}
