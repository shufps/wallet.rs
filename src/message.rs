// Copyright 2020 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::address::{Address, AddressWrapper, IotaAddress};
use bee_common::packable::Packable;
use chrono::prelude::{DateTime, Utc};
use getset::{Getters, Setters};
pub use iota::{
    Essence, IndexationPayload, Input, Message as IotaMessage, MessageId, MilestonePayload, Output, Payload,
    ReceiptPayload, RegularEssence, SignatureLockedDustAllowanceOutput, SignatureLockedSingleOutput,
    TransactionPayload, TreasuryOutput, TreasuryTransactionPayload, UnlockBlock,
};
use serde::{de::Deserializer, Deserialize, Serialize};
use serde_repr::Deserialize_repr;
use std::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
    num::NonZeroU64,
    unimplemented,
};

/// The strategy to use for the remainder value management when sending funds.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "strategy", content = "value")]
pub enum RemainderValueStrategy {
    /// Keep the remainder value on the source address.
    ReuseAddress,
    /// Move the remainder value to a change address.
    ChangeAddress,
    /// Move the remainder value to an address that must belong to the source account.
    #[serde(with = "crate::serde::iota_address_serde")]
    AccountAddress(AddressWrapper),
}

impl Default for RemainderValueStrategy {
    fn default() -> Self {
        Self::ChangeAddress
    }
}

/// A transfer to make a transaction.
#[derive(Debug, Clone)]
pub struct TransferBuilder {
    /// The transfer value.
    amount: NonZeroU64,
    /// The transfer address.
    address: AddressWrapper,
    /// (Optional) message indexation.
    indexation: Option<IndexationPayload>,
    /// The strategy to use for the remainder value.
    remainder_value_strategy: RemainderValueStrategy,
}

impl<'de> Deserialize<'de> for TransferBuilder {
    fn deserialize<D>(deserializer: D) -> Result<TransferBuilder, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Debug, Clone, Deserialize)]
        #[serde(untagged)]
        enum TransactionIndexation {
            String(String),
            Raw(Vec<u8>),
        }
        /// The message's index builder.
        #[derive(Debug, Clone, Deserialize)]
        struct IndexationPayloadBuilder {
            index: TransactionIndexation,
            data: Option<Vec<u8>>,
        }

        impl IndexationPayloadBuilder {
            /// Builds the indexation.
            pub fn finish(self) -> crate::Result<IndexationPayload> {
                let indexation = IndexationPayload::new(
                    &match self.index {
                        TransactionIndexation::String(value) => value.as_bytes().to_vec(),
                        TransactionIndexation::Raw(bytes) => bytes,
                    },
                    &self.data.unwrap_or_default(),
                )?;
                Ok(indexation)
            }
        }

        #[derive(Debug, Clone, Deserialize)]
        pub struct TransferBuilderWrapper {
            /// The transfer value.
            amount: NonZeroU64,
            /// The transfer address.
            #[serde(with = "crate::serde::iota_address_serde")]
            address: AddressWrapper,
            /// (Optional) message indexation.
            indexation: Option<IndexationPayloadBuilder>,
            /// The strategy to use for the remainder value.
            remainder_value_strategy: RemainderValueStrategy,
        }

        TransferBuilderWrapper::deserialize(deserializer).and_then(|builder| {
            Ok(TransferBuilder {
                amount: builder.amount,
                address: builder.address,
                indexation: match builder.indexation {
                    Some(i) => Some(i.finish().map_err(serde::de::Error::custom)?),
                    None => None,
                },
                remainder_value_strategy: builder.remainder_value_strategy,
            })
        })
    }
}

impl TransferBuilder {
    /// Initialises a new transfer to the given address.
    pub fn new(address: AddressWrapper, amount: NonZeroU64) -> Self {
        Self {
            address,
            amount,
            indexation: None,
            remainder_value_strategy: RemainderValueStrategy::ChangeAddress,
        }
    }

    /// Sets the remainder value strategy for the transfer.
    pub fn with_remainder_value_strategy(mut self, strategy: RemainderValueStrategy) -> Self {
        self.remainder_value_strategy = strategy;
        self
    }

    /// (Optional) message indexation.
    pub fn with_indexation(mut self, indexation: IndexationPayload) -> Self {
        self.indexation = Some(indexation);
        self
    }

    /// Builds the transfer.
    pub fn finish(self) -> Transfer {
        Transfer {
            address: self.address,
            amount: self.amount,
            indexation: self.indexation,
            remainder_value_strategy: self.remainder_value_strategy,
        }
    }
}

/// A transfer to make a transaction.
#[derive(Debug, Clone, Deserialize)]
pub struct Transfer {
    /// The transfer value.
    pub(crate) amount: NonZeroU64,
    /// The transfer address.
    #[serde(with = "crate::serde::iota_address_serde")]
    pub(crate) address: AddressWrapper,
    /// (Optional) message indexation.
    pub(crate) indexation: Option<IndexationPayload>,
    /// The strategy to use for the remainder value.
    pub(crate) remainder_value_strategy: RemainderValueStrategy,
}

impl Transfer {
    /// Initialises the transfer builder.
    pub fn builder(address: AddressWrapper, amount: NonZeroU64) -> TransferBuilder {
        TransferBuilder::new(address, amount)
    }
}

/// Possible Value units.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ValueUnit {
    /// i
    I,
    /// Ki
    Ki,
    /// Mi
    Mi,
    /// Gi
    Gi,
    /// Ti
    Ti,
    /// Pi
    Pi,
}

impl fmt::Display for ValueUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            ValueUnit::I => write!(f, "i"),
            ValueUnit::Ki => write!(f, "Ki"),
            ValueUnit::Mi => write!(f, "Mi"),
            ValueUnit::Gi => write!(f, "Gi"),
            ValueUnit::Ti => write!(f, "Ti"),
            ValueUnit::Pi => write!(f, "Pi"),
        }
    }
}

/// The transaction Value struct.
#[derive(Debug, Getters, Serialize, Deserialize, Clone)]
#[getset(get = "pub")]
pub struct Value {
    /// The value.
    value: u64,
    /// The value's unit.
    unit: ValueUnit,
}

impl Value {
    /// Ititialises a new Value.
    pub fn new(value: u64, unit: ValueUnit) -> Self {
        Self { value, unit }
    }

    /// Formats the value with its unit.
    pub fn with_denomination(&self) -> String {
        format!("{} {}", self.value, self.unit)
    }

    /// The transaction value without its unit.
    pub fn without_denomination(&self) -> u64 {
        let multiplier = match self.unit {
            ValueUnit::I => 1,
            ValueUnit::Ki => 1000,
            ValueUnit::Mi => 1000000,
            ValueUnit::Gi => 1000000000,
            ValueUnit::Ti => 1000000000000,
            ValueUnit::Pi => 1000000000000000,
        };
        self.value * multiplier
    }
}

/// Signature locked single output.
#[derive(Debug, Clone, Serialize, Deserialize, Getters, Eq, PartialEq)]
#[getset(get = "pub")]
pub struct TransactionSignatureLockedSingleOutput {
    /// The output adrress.
    #[serde(with = "crate::serde::iota_address_serde")]
    address: AddressWrapper,
    /// The output amount.
    amount: u64,
}

impl TransactionSignatureLockedSingleOutput {
    fn new(output: &SignatureLockedSingleOutput, bech32_hrp: String) -> Self {
        Self {
            address: AddressWrapper::new(*output.address(), bech32_hrp),
            amount: output.amount(),
        }
    }
}

/// Dust allowance output.
#[derive(Debug, Clone, Serialize, Deserialize, Getters, Eq, PartialEq)]
#[getset(get = "pub")]
pub struct TransactionSignatureLockedDustAllowanceOutput {
    /// The output adrress.
    #[serde(with = "crate::serde::iota_address_serde")]
    address: AddressWrapper,
    /// The output amount.
    amount: u64,
}

impl TransactionSignatureLockedDustAllowanceOutput {
    fn new(output: &SignatureLockedDustAllowanceOutput, bech32_hrp: String) -> Self {
        Self {
            address: AddressWrapper::new(*output.address(), bech32_hrp),
            amount: output.amount(),
        }
    }
}

/// The transaction output.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum TransactionOutput {
    /// Signature locked single output.
    SignatureLockedSingle(TransactionSignatureLockedSingleOutput),
    /// Dust allowance output.
    SignatureLockedDustAllowance(TransactionSignatureLockedDustAllowanceOutput),
    /// Trasury output.
    Treasury(TreasuryOutput),
}

impl TransactionOutput {
    fn new(output: &Output, bech32_hrp: String) -> Self {
        match output {
            Output::SignatureLockedSingle(output) => {
                Self::SignatureLockedSingle(TransactionSignatureLockedSingleOutput::new(output, bech32_hrp))
            }
            Output::SignatureLockedDustAllowance(output) => Self::SignatureLockedDustAllowance(
                TransactionSignatureLockedDustAllowanceOutput::new(output, bech32_hrp),
            ),
            Output::Treasury(output) => Self::Treasury(output.clone()),
            _ => unimplemented!(),
        }
    }
}

/// Regular essence.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct TransactionRegularEssence {
    inputs: Box<[Input]>,
    outputs: Box<[TransactionOutput]>,
    payload: Option<Payload>,
}

impl TransactionRegularEssence {
    /// Gets the transaction inputs.
    pub fn inputs(&self) -> &[Input] {
        &self.inputs
    }

    /// Gets the transaction outputs.
    pub fn outputs(&self) -> &[TransactionOutput] {
        &self.outputs
    }

    /// Gets the transaction chained payload.
    pub fn payload(&self) -> &Option<Payload> {
        &self.payload
    }
}

impl TransactionRegularEssence {
    fn new(regular_essence: &RegularEssence, bech32_hrp: String) -> Self {
        let mut inputs = Vec::new();
        for input in regular_essence.inputs() {
            inputs.push(input.clone());
        }
        let mut outputs = Vec::new();
        for output in regular_essence.outputs() {
            outputs.push(TransactionOutput::new(output, bech32_hrp.clone()));
        }
        Self {
            inputs: inputs.into_boxed_slice(),
            outputs: outputs.into_boxed_slice(),
            payload: regular_essence.payload().clone(),
        }
    }
}

/// The transaction essence.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum TransactionEssence {
    /// Regular essence type.
    Regular(TransactionRegularEssence),
}

impl TransactionEssence {
    #[doc(hidden)]
    pub fn new(essence: &Essence, bech32_hrp: String) -> Self {
        match essence {
            Essence::Regular(regular) => Self::Regular(TransactionRegularEssence::new(regular, bech32_hrp)),
            _ => unimplemented!(),
        }
    }
}

/// A transaction payload.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct MessageTransactionPayload {
    essence: TransactionEssence,
    unlock_blocks: Box<[UnlockBlock]>,
}

impl MessageTransactionPayload {
    /// The transaction essence.
    pub fn essence(&self) -> &TransactionEssence {
        &self.essence
    }

    fn essence_mut(&mut self) -> &mut TransactionEssence {
        &mut self.essence
    }

    /// The unlock blocks.
    pub fn unlock_blocks(&self) -> &[UnlockBlock] {
        &self.unlock_blocks
    }

    #[doc(hidden)]
    pub fn new(payload: &TransactionPayload, bech32_hrp: String) -> Self {
        let mut unlock_blocks = Vec::new();
        for unlock_block in payload.unlock_blocks() {
            unlock_blocks.push(unlock_block.clone());
        }
        Self {
            essence: TransactionEssence::new(payload.essence(), bech32_hrp),
            unlock_blocks: unlock_blocks.into_boxed_slice(),
        }
    }
}

/// The message's payload.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum MessagePayload {
    /// Transaction payload.
    Transaction(Box<MessageTransactionPayload>),
    /// Milestone payload.
    Milestone(Box<MilestonePayload>),
    /// Indexation payload.
    Indexation(Box<IndexationPayload>),
    /// Receipt payload.
    Receipt(Box<ReceiptPayload>),
    /// Treasury Transaction payload.
    TreasuryTransaction(Box<TreasuryTransactionPayload>),
}

impl MessagePayload {
    pub(crate) fn new(payload: Payload, bech32_hrp: String) -> Self {
        match payload {
            Payload::Transaction(tx) => Self::Transaction(Box::new(MessageTransactionPayload::new(&tx, bech32_hrp))),
            Payload::Milestone(milestone) => Self::Milestone(milestone),
            Payload::Indexation(indexation) => Self::Indexation(indexation),
            Payload::Receipt(receipt) => Self::Receipt(receipt),
            Payload::TreasuryTransaction(treasury_tx) => Self::TreasuryTransaction(treasury_tx),
            _ => unimplemented!(),
        }
    }
}

/// A message definition.
#[derive(Debug, Getters, Setters, Clone, Serialize, Deserialize, Eq)]
#[getset(get = "pub", set = "pub(crate)")]
pub struct Message {
    /// The message identifier.
    pub id: MessageId,
    /// The message version.
    pub version: u64,
    /// Message ids this message refers to.
    pub parents: Vec<MessageId>,
    /// Length of the payload.
    #[serde(rename = "payloadLength")]
    pub payload_length: usize,
    /// Message payload.
    pub payload: Option<MessagePayload>,
    /// The transaction timestamp.
    pub timestamp: DateTime<Utc>,
    /// Transaction nonce.
    pub nonce: u64,
    /// Whether the transaction is confirmed or not.
    #[getset(set = "pub")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmed: Option<bool>,
    /// Whether the transaction is broadcasted or not.
    #[getset(set = "pub")]
    pub broadcasted: bool,
    /// Whether the message represents an incoming transaction or not.
    pub incoming: bool,
    /// The message's value.
    pub value: u64,
    /// The message's remainder value sum.
    #[serde(rename = "remainderValue")]
    pub remainder_value: u64,
}

impl Message {
    pub(crate) fn set_bech32_hrp(&mut self, bech32_hrp: String) {
        if let Some(MessagePayload::Transaction(tx)) = self.payload.as_mut() {
            match tx.essence_mut() {
                TransactionEssence::Regular(essence) => {
                    for output in essence.outputs.iter_mut() {
                        match output {
                            TransactionOutput::SignatureLockedSingle(output) => {
                                output.address.bech32_hrp = bech32_hrp.clone();
                            }
                            TransactionOutput::SignatureLockedDustAllowance(output) => {
                                output.address.bech32_hrp = bech32_hrp.clone();
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

impl Hash for Message {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

impl PartialEq for Message {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Ord for Message {
    fn cmp(&self, other: &Self) -> Ordering {
        self.id.as_ref().cmp(&other.id.as_ref())
    }
}

impl PartialOrd for Message {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub(crate) struct MessageBuilder<'a> {
    id: MessageId,
    iota_message: IotaMessage,
    account_addresses: &'a [Address],
    confirmed: Option<bool>,
    bech32_hrp: String,
}

impl<'a> MessageBuilder<'a> {
    pub fn new(id: MessageId, iota_message: IotaMessage, account_addresses: &'a [Address], bech32_hrp: String) -> Self {
        Self {
            id,
            iota_message,
            account_addresses,
            confirmed: None,
            bech32_hrp,
        }
    }

    pub fn with_confirmed(mut self, confirmed: Option<bool>) -> Self {
        self.confirmed = confirmed;
        self
    }

    /// Gets the absolute value of the transaction.
    pub fn compute_value(iota_message: &IotaMessage, id: &MessageId, account_addresses: &[Address]) -> Value {
        let amount = match iota_message.payload().as_ref() {
            Some(Payload::Transaction(tx)) => {
                let essence = tx.essence();
                let outputs = match essence {
                    Essence::Regular(essence) => essence.outputs(),
                    _ => unimplemented!(),
                };
                let outputs: Vec<(&IotaAddress, u64)> = outputs
                    .iter()
                    .map(|output| match output {
                        Output::SignatureLockedDustAllowance(o) => (o.address(), o.amount()),
                        Output::SignatureLockedSingle(o) => (o.address(), o.amount()),
                        _ => unimplemented!(),
                    })
                    .collect();
                // if all outputs belongs to the account, we can't determine whether this transfer is incoming or
                // outgoing; so we assume that the highest address index holds the remainder, and the rest is the
                // transfer outputs
                let all_outputs_belongs_to_account = outputs.iter().all(|(address, _)| {
                    let address_belongs_to_account = account_addresses.iter().any(|a| &a.address().as_ref() == address);
                    address_belongs_to_account
                });
                if all_outputs_belongs_to_account {
                    let mut remainder = (None, 0); // (address_index, amount)
                    let mut total = 0;
                    for (address, amount) in outputs {
                        total += amount;
                        let account_address = account_addresses
                            .iter()
                            .find(|a| a.address().as_ref() == address)
                            .unwrap(); // safe to unwrap since we already asserted that the address belongs to the account
                        match remainder.0 {
                            Some(index) => {
                                let address_index = *account_address.key_index();
                                // if the address index is the highest or it's the same as the previous one and this is
                                // a change address, we assume that it holds the
                                // remainder value
                                if address_index > index || (address_index == index && *account_address.internal()) {
                                    remainder = (Some(*account_address.key_index()), amount);
                                }
                            }
                            None => {
                                remainder = (Some(*account_address.key_index()), amount);
                            }
                        }
                    }
                    total - remainder.1
                } else {
                    let sent = !account_addresses
                        .iter()
                        .any(|address| address.outputs().iter().any(|o| o.message_id() == id));
                    outputs.iter().fold(0, |acc, (address, amount)| {
                        let address_belongs_to_account =
                            account_addresses.iter().any(|a| &a.address().as_ref() == address);
                        if sent {
                            if address_belongs_to_account {
                                acc
                            } else {
                                acc + amount
                            }
                        } else if address_belongs_to_account {
                            acc + amount
                        } else {
                            acc
                        }
                    })
                }
            }
            _ => 0,
        };
        Value::new(amount, ValueUnit::I)
    }

    pub fn finish(self) -> Message {
        let packed_payload = self.iota_message.payload().pack_new();

        let (value, remainder_value) = {
            let total_value = match self.iota_message.payload().as_ref() {
                Some(Payload::Transaction(tx)) => match tx.essence() {
                    Essence::Regular(essence) => essence.outputs().iter().fold(0, |acc, output| {
                        acc + match output {
                            Output::SignatureLockedDustAllowance(o) => o.amount(),
                            Output::SignatureLockedSingle(o) => o.amount(),
                            _ => 0,
                        }
                    }),
                    _ => unimplemented!(),
                },
                _ => 0,
            };
            let value =
                Self::compute_value(&self.iota_message, &self.id, &self.account_addresses).without_denomination();
            (value, total_value - value)
        };
        let bech32_hrp = self.bech32_hrp.to_string();

        Message {
            id: self.id,
            version: 1,
            parents: self.iota_message.parents().to_vec(),
            payload_length: packed_payload.len(),
            payload: self
                .iota_message
                .payload()
                .clone()
                .map(|p| MessagePayload::new(p, bech32_hrp)),
            timestamp: Utc::now(),
            nonce: self.iota_message.nonce(),
            confirmed: self.confirmed,
            broadcasted: true,
            incoming: self
                .account_addresses
                .iter()
                .any(|address| address.outputs().iter().any(|o| o.message_id() == &self.id)),
            value,
            remainder_value,
        }
    }
}

impl Message {
    pub(crate) fn from_iota_message(
        id: MessageId,
        iota_message: IotaMessage,
        account_addresses: &'_ [Address],
    ) -> MessageBuilder<'_> {
        MessageBuilder::new(
            id,
            iota_message,
            account_addresses,
            account_addresses
                .iter()
                .next()
                .unwrap()
                .address()
                .bech32_hrp()
                .to_string(),
        )
    }

    /// The message's addresses.
    pub fn addresses(&self) -> Vec<&AddressWrapper> {
        match &self.payload {
            Some(MessagePayload::Transaction(tx)) => match tx.essence() {
                TransactionEssence::Regular(essence) => essence
                    .outputs()
                    .iter()
                    .map(|output| match output {
                        TransactionOutput::SignatureLockedDustAllowance(o) => o.address(),
                        TransactionOutput::SignatureLockedSingle(o) => o.address(),
                        _ => unimplemented!(),
                    })
                    .collect(),
            },

            _ => vec![],
        }
    }
}

/// Message type.
#[derive(Debug, Clone, Deserialize_repr, PartialEq)]
#[repr(u8)]
pub enum MessageType {
    /// Message received.
    Received = 1,
    /// Message sent.
    Sent = 2,
    /// Message not broadcasted.
    Failed = 3,
    /// Message not confirmed.
    Unconfirmed = 4,
    /// A value message.
    Value = 5,
}
