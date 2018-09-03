use beserial::{Serialize, Deserialize};
use consensus::base::primitive::Address;
use consensus::base::transaction::{Transaction, SignatureProof};
use super::{Account, AccountError};
use std::io;

#[derive(Clone, PartialEq, PartialOrd, Eq, Ord, Debug, Serialize, Deserialize)]
pub struct VestingContract {
    pub balance: u64,
    pub owner: Address,
    pub vesting_start: u32,
    pub vesting_step_blocks: u32,
    pub vesting_step_amount: u64,
    pub vesting_total_amount: u64
}

impl VestingContract {
    pub fn create(balance: u64, transaction: &Transaction, block_height: u32) -> Result<Self, AccountError> {
        return match VestingContract::create_from_transaction(balance, transaction) {
            Ok(account) => Ok(account),
            Err(_) => Err(AccountError("Failed to create vesting contract".to_string()))
        };
    }

    fn create_from_transaction(balance: u64, transaction: &Transaction) -> io::Result<Self> {
        let reader = &mut &transaction.data[..];
        let owner = Deserialize::deserialize(reader)?;

        if transaction.data.len() == Address::SIZE + 4 {
            // Only block number: vest full amount at that block
            let vesting_step_blocks = Deserialize::deserialize(reader)?;
            return Ok(VestingContract::new(balance, owner, 0, vesting_step_blocks, transaction.value, transaction.value));
        }
        else if transaction.data.len() == Address::SIZE + 16 {
            let vesting_start = Deserialize::deserialize(reader)?;
            let vesting_step_blocks = Deserialize::deserialize(reader)?;
            let vesting_step_amount = Deserialize::deserialize(reader)?;
            return Ok(VestingContract::new(balance, owner, vesting_start, vesting_step_blocks, vesting_step_amount, transaction.value));
        }
        else if transaction.data.len() == Address::SIZE + 24 {
            // Create a vesting account with some instantly vested funds or additional funds considered.
            let vesting_start = Deserialize::deserialize(reader)?;
            let vesting_step_blocks = Deserialize::deserialize(reader)?;
            let vesting_step_amount = Deserialize::deserialize(reader)?;
            let vesting_total_amount = Deserialize::deserialize(reader)?;
            return Ok(VestingContract::new(balance, owner, vesting_start, vesting_step_blocks, vesting_step_amount, vesting_total_amount));
        }
        else {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid transaction data"));
        }
    }

    fn new(balance: u64, owner: Address, vesting_start: u32, vesting_step_blocks: u32, vesting_step_amount: u64, vesting_total_amount: u64) -> Self {
        return VestingContract { balance, owner, vesting_start, vesting_step_blocks, vesting_step_amount, vesting_total_amount };
    }

    pub fn verify_incoming_transaction(transaction: &Transaction) -> bool {
        // The contract creation transaction is the only valid incoming transaction.
        if transaction.recipient != transaction.contract_creation_address() {
            return false;
        }

        // Check that data field has the correct length.
        let allowed_sizes = [Address::SIZE + 4, Address::SIZE + 16, Address::SIZE + 24];
        return allowed_sizes.contains(&transaction.data.len());
    }

    pub fn verify_outgoing_transaction(transaction: &Transaction) -> bool {
        let signature_proof: SignatureProof = match Deserialize::deserialize(&mut &transaction.proof[..]) {
            Ok(v) => v,
            Err(e) => return false
        };
        return signature_proof.verify(transaction.serialize_content().as_slice());
    }

    fn with_balance(&self, balance: u64) -> Self {
        return VestingContract {
            balance,
            owner: self.owner.clone(),
            vesting_start: self.vesting_start,
            vesting_step_blocks: self.vesting_step_blocks,
            vesting_step_amount: self.vesting_step_amount,
            vesting_total_amount: self.vesting_total_amount
        };
    }

    pub fn with_incoming_transaction(&self, transaction: &Transaction, block_height: u32) -> Result<Self, AccountError> {
        return Err(AccountError("Illegal incoming transaction".to_string()));
    }

    pub fn without_incoming_transaction(&self, transaction: &Transaction, block_height: u32) -> Result<Self, AccountError> {
        return Err(AccountError("Illegal incoming transaction".to_string()));
    }

    pub fn with_outgoing_transaction(&self, transaction: &Transaction, block_height: u32) -> Result<Self, AccountError> {
        let balance: u64 = Account::balance_sub(self.balance, transaction.value + transaction.fee)?;

        // Check vesting min cap.
        if balance < self.min_cap(block_height) {
            return Err(AccountError("Insufficient funds available".to_string()));
        }

        // Check transaction signer is contract owner.
        let signature_proof: SignatureProof = match Deserialize::deserialize(&mut &transaction.proof[..]) {
            Ok(v) => v,
            Err(e) => return Err(AccountError("Invalid proof".to_string()))
        };
        if !signature_proof.is_signed_by(&self.owner) {
            return Err(AccountError("Invalid signer".to_string()));
        }

        return Ok(self.with_balance(balance));
    }

    pub fn without_outgoing_transaction(&self, transaction: &Transaction, block_height: u32) -> Result<Self, AccountError> {
        let balance: u64 = Account::balance_add(self.balance, transaction.value + transaction.fee)?;
        return Ok(self.with_balance(balance));
    }

    pub fn min_cap(&self, block_height: u32) -> u64 {
        return if self.vesting_step_blocks > 0 && self.vesting_step_amount > 0 {
            let steps = ((block_height - self.vesting_start) as f64 / self.vesting_step_blocks as f64).floor();
            let min_cap = self.vesting_total_amount as f64 - steps * self.vesting_step_amount as f64;
            min_cap.max(0f64) as u64
        } else {
            0u64
        };
    }
}