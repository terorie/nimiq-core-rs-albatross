#![allow(dead_code)]
use crate::gadgets::macro_block::{MacroBlock, MacroBlockGadget};

mod gadgets;

// Bring in some tools for using pairing-friendly curves
// We're going to use the BLS12-377 pairing-friendly elliptic curve.
use algebra::One;
use algebra::{curves::sw6::SW6, fields::bls12_377::fq::Fq, ProjectiveCurve, UniformRand, Zero};
// For randomness (during paramgen and proof generation)
use algebra::test_rng;
// We're going to use the Groth 16 proving system.
use groth16::{
    create_random_proof, generate_random_parameters, prepare_verifying_key, verify_proof,
};
// For benchmarking
use std::str::FromStr;
use std::{env, fs::OpenOptions, path::PathBuf, process};
use std::{
    error::Error,
    time::{Duration, Instant},
};

use crate::constraints::Benchmark;
use algebra::curves::bls12_377::{G1Affine, G1Projective, G2Projective};
use algebra::fields::bls12_377::Fr;
use nimiq_bls::{KeyPair, SecureGenerate};

mod constraints;

fn main() -> Result<(), Box<dyn Error>> {
    // This may not be cryptographically safe, use
    // `OsRng` (for example) in production software.
    let rng = &mut test_rng();

    let mut total_setup = Duration::new(0, 0);
    let mut total_proving = Duration::new(0, 0);
    let mut total_verifying = Duration::new(0, 0);

    let generator = G2Projective::prime_subgroup_generator();
    let key_pair = KeyPair::generate_default_csprng();
    let key_pair2 = KeyPair::generate_default_csprng();
    let genesis_keys = vec![
        key_pair.public_key.public_key,
        key_pair2.public_key.public_key,
    ];

    let signers_bitmap = vec![true, false];
    let macro_block = MacroBlock {
        header_hash: [0; 32],
        public_keys: vec![
            key_pair.public_key.public_key,
            key_pair2.public_key.public_key,
        ],
    };

    let macro_hash = macro_block.hash();
    let signature = key_pair.sign_hash(macro_hash);
    let max_non_signers = 2;

    // Create parameters for our circuit
    let start = Instant::now();
    let params = {
        let c = Benchmark::new(
            genesis_keys.clone(),
            signers_bitmap.clone(),
            macro_block.clone(),
            signature.signature,
            generator,
            max_non_signers,
        );
        generate_random_parameters::<SW6, _, _>(c, rng)?
    };

    // Prepare the verification key (for proof verification)
    let pvk = prepare_verifying_key(&params.vk);
    total_setup += start.elapsed();

    // proof_vec.truncate(0);
    let start = Instant::now();
    let proof = {
        // Create an instance of our circuit (with the witness)
        let c = Benchmark::new(
            genesis_keys.clone(),
            signers_bitmap.clone(),
            macro_block.clone(),
            signature.signature,
            generator,
            max_non_signers,
        );
        // Create a proof with our parameters.
        create_random_proof(c, &params, rng)?
    };

    total_proving += start.elapsed();

    let inputs: Vec<Fq> = vec![];

    let start = Instant::now();
    // let proof = Proof::read(&proof_vec[..]).unwrap();
    // Check the proof
    let verified = verify_proof(&pvk, &proof, &inputs).unwrap();
    total_verifying += start.elapsed();

    println!("=== Benchmarking Groth16: ====");
    println!("Result: {}", verified);
    println!(
        "Verification key size: {:?} bytes",
        336 + 48 * params.vk.gamma_abc_g1.len()
    );
    println!(
        "Verification key gamma len: {:?}",
        params.vk.gamma_abc_g1.len()
    );
    println!("Average setup time: {:?} seconds", total_setup);
    println!("Average proving time: {:?} seconds", total_proving);
    println!("Average verifying time: {:?} seconds", total_verifying);

    Ok(())
}
