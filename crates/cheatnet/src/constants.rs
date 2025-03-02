use std::sync::Arc;
use std::{collections::HashMap, fs, path::PathBuf};

use blockifier::execution::contract_class::ContractClassV1;
use blockifier::{
    abi::{abi_utils::get_storage_var_address, constants},
    block_context::BlockContext,
    execution::{
        contract_class::{ContractClass, ContractClassV0},
        execution_utils::felt_to_stark_felt,
    },
    transaction::objects::AccountTransactionContext,
};
use cairo_felt::Felt252;
use cairo_vm::vm::runners::builtin_runner::{
    BITWISE_BUILTIN_NAME, EC_OP_BUILTIN_NAME, HASH_BUILTIN_NAME, KECCAK_BUILTIN_NAME,
    OUTPUT_BUILTIN_NAME, POSEIDON_BUILTIN_NAME, RANGE_CHECK_BUILTIN_NAME, SIGNATURE_BUILTIN_NAME,
};
use camino::Utf8PathBuf;
use starknet_api::{
    block::{BlockNumber, BlockTimestamp},
    core::{ChainId, ClassHash, ContractAddress, Nonce, PatriciaKey},
    hash::{StarkFelt, StarkHash},
    patricia_key, stark_felt,
    state::StorageKey,
    transaction::{
        Calldata, DeclareTransactionV2, Fee, InvokeTransactionV1, TransactionHash,
        TransactionSignature, TransactionVersion,
    },
};

use crate::state::DictStateReader;

pub const TEST_SEQUENCER_ADDRESS: &str = "0x1000";
pub const TEST_ERC20_CONTRACT_ADDRESS: &str = "0x1001";
pub const TEST_ACCOUNT_CONTRACT_ADDRESS: &str = "0x101";
pub const MAX_FEE: u128 = 1_000_000 * 100_000_000_000; // 1000000 * min_gas_price.
pub const INITIAL_BALANCE: u128 = 10 * MAX_FEE;

// Mocked class hashes, those are not checked anywhere
pub const TEST_CLASS_HASH: &str = "0x110";
pub const TEST_ACCOUNT_CONTRACT_CLASS_HASH: &str = "0x111";
pub const TEST_EMPTY_CONTRACT_CLASS_HASH: &str = "0x112";
pub const TEST_FAULTY_ACCOUNT_CONTRACT_CLASS_HASH: &str = "0x113";
pub const SECURITY_TEST_CLASS_HASH: &str = "0x114";
pub const TEST_ERC20_CONTRACT_CLASS_HASH: &str = "0x1010";

pub const STEP_RESOURCE_COST: f64 = 0.01_f64;

// HOW TO FIND:
// 1. https://docs.starknet.io/documentation/architecture_and_concepts/Network_Architecture/fee-mechanism/#general_case
// 2. src/starkware/cairo/lang/instances.py::starknet_with_keccak_instance
#[must_use]
pub fn build_block_context() -> BlockContext {
    // blockifier::test_utils::create_for_account_testing
    let vm_resource_fee_cost = Arc::new(HashMap::from([
        (constants::N_STEPS_RESOURCE.to_string(), STEP_RESOURCE_COST),
        (HASH_BUILTIN_NAME.to_string(), 32_f64 * STEP_RESOURCE_COST),
        (
            RANGE_CHECK_BUILTIN_NAME.to_string(),
            16_f64 * STEP_RESOURCE_COST,
        ),
        (
            SIGNATURE_BUILTIN_NAME.to_string(),
            2048_f64 * STEP_RESOURCE_COST,
        ), // ECDSA
        (
            BITWISE_BUILTIN_NAME.to_string(),
            64_f64 * STEP_RESOURCE_COST,
        ),
        (
            POSEIDON_BUILTIN_NAME.to_string(),
            32_f64 * STEP_RESOURCE_COST,
        ),
        (OUTPUT_BUILTIN_NAME.to_string(), 0_f64 * STEP_RESOURCE_COST),
        (
            EC_OP_BUILTIN_NAME.to_string(),
            1024_f64 * STEP_RESOURCE_COST,
        ),
        (
            KECCAK_BUILTIN_NAME.to_string(),
            2048_f64 * STEP_RESOURCE_COST, // 2**11
        ),
        // The gas estimation should panic in case it encounters a builtin that doesn't have a cost
        // This builtin seems to be unused for cost estimation
        // (
        //     SEGMENT_ARENA_BUILTIN_NAME.to_string(),
        //     0_f64 * STEP_RESOURCE_COST,
        // ), // BUILTIN COST NOT FOUND
    ]));

    BlockContext {
        chain_id: ChainId("SN_GOERLI".to_string()),
        block_number: BlockNumber(2000),
        block_timestamp: BlockTimestamp::default(),
        sequencer_address: ContractAddress(patricia_key!(TEST_SEQUENCER_ADDRESS)),
        fee_token_address: ContractAddress(patricia_key!(TEST_ERC20_CONTRACT_ADDRESS)),
        deprecated_fee_token_address: ContractAddress(patricia_key!(TEST_ERC20_CONTRACT_ADDRESS)),
        vm_resource_fee_cost,
        gas_price: 100 * u128::pow(10, 9),
        invoke_tx_max_n_steps: 1_000_000,
        validate_max_n_steps: 1_000_000,
        max_recursion_depth: 50,
    }
}

#[must_use]
pub fn build_transaction_context() -> AccountTransactionContext {
    AccountTransactionContext {
        transaction_hash: TransactionHash::default(),
        max_fee: Fee::default(),
        version: TransactionVersion(StarkFelt::from(2_u8)),
        signature: TransactionSignature::default(),
        nonce: Nonce(felt_to_stark_felt(&Felt252::from(0))),
        sender_address: ContractAddress::default(),
    }
}

#[must_use]
pub fn build_declare_transaction(
    nonce: Nonce,
    class_hash: ClassHash,
    sender_address: ContractAddress,
) -> DeclareTransactionV2 {
    DeclareTransactionV2 {
        nonce,
        class_hash,
        sender_address,
        ..Default::default()
    }
}

#[must_use]
pub fn build_invoke_transaction(
    calldata: Calldata,
    sender_address: ContractAddress,
) -> InvokeTransactionV1 {
    InvokeTransactionV1 {
        sender_address,
        calldata,
        ..Default::default()
    }
}

fn read_predeployed_contract_file(
    predeployed_contracts: &Utf8PathBuf,
    contract_path: &str,
) -> String {
    let full_contract_path: PathBuf = predeployed_contracts.join(contract_path).into();
    fs::read_to_string(full_contract_path).expect("Failed to read predeployed contracts")
}

fn load_v0_contract_class(
    predeployed_contracts: &Utf8PathBuf,
    contract_path: &str,
) -> ContractClassV0 {
    let raw_contract_class = read_predeployed_contract_file(predeployed_contracts, contract_path);
    ContractClassV0::try_from_json_string(&raw_contract_class).unwrap()
}

fn load_v1_contract_class(
    predeployed_contracts: &Utf8PathBuf,
    contract_path: &str,
) -> ContractClassV1 {
    let raw_contract_class = read_predeployed_contract_file(predeployed_contracts, contract_path);
    ContractClassV1::try_from_json_string(&raw_contract_class).unwrap()
}

fn erc20_account_balance_key() -> StorageKey {
    get_storage_var_address(
        "ERC20_balances",
        &[stark_felt!(TEST_ACCOUNT_CONTRACT_ADDRESS)],
    )
    .unwrap()
}

// Creates a state with predeployed account and erc20 used to send transactions during tests.
// Deployed contracts are cairo 0 contracts
// Account does not include validations
#[must_use]
pub fn build_testing_state(predeployed_contracts: &Utf8PathBuf) -> DictStateReader {
    let account_class = load_v1_contract_class(predeployed_contracts, "account_cairo1.casm.json");
    let erc20_class = load_v0_contract_class(
        predeployed_contracts,
        "erc20_contract_without_some_syscalls_compiled.json",
    );
    let block_context = build_block_context();
    let test_account_class_hash = ClassHash(stark_felt!(TEST_ACCOUNT_CONTRACT_CLASS_HASH));
    let test_erc20_class_hash = ClassHash(stark_felt!(TEST_ERC20_CONTRACT_CLASS_HASH));

    let class_hash_to_class = HashMap::from([
        (test_account_class_hash, ContractClass::V1(account_class)),
        (test_erc20_class_hash, ContractClass::V0(erc20_class)),
    ]);

    // A random address that is unlikely to equal the result of the calculation of a contract
    // address.
    let test_account_address = ContractAddress(patricia_key!(TEST_ACCOUNT_CONTRACT_ADDRESS));
    let test_erc20_address = block_context.fee_token_address;
    let address_to_class_hash = HashMap::from([
        (test_account_address, test_account_class_hash),
        (test_erc20_address, test_erc20_class_hash),
    ]);
    let address_to_nonce = HashMap::from([(test_account_address, Nonce(StarkFelt::from(0_u8)))]);
    let minter_var_address = get_storage_var_address("permitted_minter", &[])
        .expect("Failed to get permitted_minter storage address.");
    let storage_view = HashMap::from([
        (
            (test_erc20_address, erc20_account_balance_key()),
            stark_felt!(INITIAL_BALANCE),
        ),
        // Give the account mint permission.
        (
            (test_erc20_address, minter_var_address),
            *test_account_address.0.key(),
        ),
    ]);

    DictStateReader {
        storage_view,
        address_to_nonce,
        address_to_class_hash,
        class_hash_to_class,
        ..Default::default()
    }
}
