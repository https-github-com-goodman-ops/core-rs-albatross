use std::convert::{TryFrom, TryInto};

use beserial::{Deserialize, Serialize};
use nimiq_account::AccountType;
use nimiq_bls::bls12_381::KeyPair as BlsKeyPair;
use nimiq_keys::{Address, KeyPair, PrivateKey};
use nimiq_primitives::coin::Coin;
use nimiq_primitives::networks::NetworkId;
use nimiq_transaction::{SignatureProof, Transaction};
use nimiq_transaction::account::staking_contract::{StakingTransactionData, StakingTransactionType};
use nimiq_transaction_builder::{TransactionBuilder, Recipient};

#[test]
fn it_can_verify_staking_transaction() {
    let bls_pair = bls_key_pair();
    let key_pair = ed25519_key_pair();
    let address = Address::from(&key_pair);
    let mut tx = make_incoming_transaction();

    let proof_of_knowledge = bls_pair.sign(&bls_pair.public).compress();

    let data = StakingTransactionData {
        validator_key: bls_pair.public.compress(),
        reward_address: Some(Address::from([3u8; 20])),
        proof_of_knowledge,
    };
    tx.data = data.serialize_to_vec();
    tx.sender = address;
    let signature = key_pair.sign(tx.serialize_content().as_slice());
    let signature = SignatureProof::from(key_pair.public, signature);
    tx.proof = signature.serialize_to_vec();

    let tx2 = TransactionBuilder::new_staking(&key_pair,
                                                      Address::from([1u8; 20]),
                                                      &bls_pair,
                                                      150_000_000.try_into().unwrap(),
                                                      234.try_into().unwrap(),
                                                      Some(Address::from([3u8; 20])),
                                                      1,
                                                      NetworkId::Dummy);

    assert_eq!(tx2, tx);
}

#[test]
fn it_can_verify_retire_transaction() {
    let mut tx = make_outgoing_transaction();
    tx.recipient = tx.sender.clone();
    tx.recipient_type = AccountType::Staking;
    tx.data = StakingTransactionType::Retire.serialize_to_vec();

    let key_pair = ed25519_key_pair();
    let signature = key_pair.sign(tx.serialize_content().as_slice());
    let signature = SignatureProof::from(key_pair.public, signature);
    tx.proof = signature.serialize_to_vec();

    let tx2 = TransactionBuilder::new_retire(&key_pair,
                                              Address::from([1u8; 20]),
                                             149_999_766.try_into().unwrap(),
                                              234.try_into().unwrap(),
                                              1,
                                              NetworkId::Dummy);

    assert_eq!(tx2, tx);
}

#[test]
fn it_can_verify_unstaking_transaction() {
    let mut tx = make_outgoing_transaction();

    let key_pair = ed25519_key_pair();
    let signature = key_pair.sign(tx.serialize_content().as_slice());
    let signature = SignatureProof::from(key_pair.public, signature);
    tx.proof = signature.serialize_to_vec();

    let mut builder = TransactionBuilder::new();
    builder.with_sender(Address::from([1u8; 20]))
        .with_sender_type(AccountType::Staking)
        .with_recipient(Recipient::new_basic(Address::from([2u8; 20])))
        .with_value(149_999_766.try_into().unwrap())
        .with_fee(234.try_into().unwrap())
        .with_validity_start_height(1)
        .with_network_id(NetworkId::Dummy);
    let proof_builder = builder.generate()
        .expect("Builder should be able to create transaction");
    let mut proof_builder = proof_builder.unwrap_basic();
    proof_builder.sign_with_key_pair(&key_pair);

    assert_eq!(proof_builder.generate().unwrap(), tx);
}

#[test]
fn it_can_apply_unpark_transactions() {
    let key_pair = ed25519_key_pair();

    let mut tx = make_outgoing_transaction();
    tx.recipient = tx.sender.clone();
    tx.recipient_type = AccountType::Staking;
    tx.value = Coin::try_from(149_999_766).unwrap();
    tx.fee = Coin::try_from(234).unwrap();
    tx.data = StakingTransactionType::Unpark.serialize_to_vec();
    tx.proof = SignatureProof::from(key_pair.public, key_pair.sign(&tx.serialize_content())).serialize_to_vec();

    let tx2 = TransactionBuilder::new_unpark(&key_pair,
                                             Address::from([1u8; 20]),
                                             150_000_000.try_into().unwrap(),
                                             234.try_into().unwrap(),
                                             1,
                                             NetworkId::Dummy);

    assert_eq!(tx2, tx);
}

fn bls_key_pair() -> BlsKeyPair {
    const BLS_PRIVKEY: &str = "30a891c851e27600fefa7b0a84eac9caa645c98f2790e715fa09e49cb34fd73c";
    BlsKeyPair::from_secret(&Deserialize::deserialize(&mut &hex::decode(BLS_PRIVKEY).unwrap()[..]).unwrap())
}

fn ed25519_key_pair() -> KeyPair {
    const PRIVKEY: &str = "fc9b15259bf42d3e7415b75a41db8e3280280bffa7ffbe5903a5537ac9b45f75";
    let priv_key: PrivateKey = Deserialize::deserialize(&mut &hex::decode(PRIVKEY).unwrap()[..]).unwrap();
    priv_key.into()
}

fn make_incoming_transaction() -> Transaction {
    let mut tx = Transaction::new_basic(
        Address::from([2u8; 20]),
        Address::from([1u8; 20]),
        150_000_000.try_into().unwrap(),
        234.try_into().unwrap(),
        1, NetworkId::Dummy,
    );
    tx.recipient_type = AccountType::Staking;
    tx
}

fn make_outgoing_transaction() -> Transaction {
    let mut tx = Transaction::new_basic(
        Address::from([1u8; 20]),
        Address::from([2u8; 20]),
        149_999_766.try_into().unwrap(),
        234.try_into().unwrap(),
        1, NetworkId::Dummy,
    );
    tx.sender_type = AccountType::Staking;
    tx
}
