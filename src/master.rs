/*
* Copyright (C) 2019-2024 EverX. All Rights Reserved.
*
* Licensed under the SOFTWARE EVALUATION License (the "License"); you may not use
* this file except in compliance with the License.
*
* Unless required by applicable law or agreed to in writing, software
* distributed under the License is distributed on an "AS IS" BASIS,
* WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
* See the License for the specific EVERX DEV software governing permissions and
* limitations under the License.
*/

use crate::{
    bintree::{BinTree, BinTreeType},
    blocks::{Block, BlockIdExt, ExtBlkRef, ProofChain},
    config_params::ConfigParams,
    define_HashmapAugE, define_HashmapE,
    dictionary::hashmapaug::{Augmentable, HashmapAugType, TraverseNextStep},
    error::BlockError, HashUpdate,
    inbound_messages::InMsg,
    shard::{AccountIdPrefixFull, ShardIdent, SHARD_FULL},
    signature::CryptoSignaturePair,
    types::{ChildCell, CurrencyCollection, InRefValue},
    validators::{ValidatorInfo, ValidatorsStat}, VarUInteger32,
    CopyleftRewards, Deserializable, Serializable, U15, Augmentation,
    error, fail, hm_label, AccountId, BuilderData, Cell, IBitstring, Result, MsgPackId,
    SERDE_OPTS_COMMON_MESSAGE, SERDE_OPTS_EMPTY, SERDE_OPTS_MEMPOOL_NODES, SliceData, UInt256,
};
use std::{collections::HashMap, fmt, ops::Range};

#[cfg(test)]
#[path = "tests/test_master.rs"]
mod tests;

/*
_ (HashmapE 32 ^(BinTree ShardDescr)) = ShardHashes;
_ (HashmapAugE 96 ShardFeeCreated ShardFeeCreated) = ShardFees;
*/
define_HashmapE!{ShardHashes, 32, InRefValue<BinTree<ShardDescr>>}
define_HashmapE!{CryptoSignatures, 16, CryptoSignaturePair}
define_HashmapAugE!{ShardFees, 96, ShardIdentFull, ShardFeeCreated, ShardFeeCreated}

impl Augmentation<ShardFeeCreated> for ShardFeeCreated {
    fn aug(&self) -> Result<ShardFeeCreated> {
        Ok(self.clone())
    }
}

#[derive(Clone, Debug, Default)]
pub struct ShardIdentFull {
    pub workchain_id: i32,
    pub prefix: u64, // with terminated bit!
}

impl ShardIdentFull {
    pub fn new(workchain_id: i32, prefix: u64) -> ShardIdentFull {
        ShardIdentFull {
            workchain_id,
            prefix,
        }
    }
}

impl Serializable for ShardIdentFull {
    fn write_to(&self, cell: &mut BuilderData) -> Result<()> {
        self.workchain_id.write_to(cell)?;
        self.prefix.write_to(cell)?;
        Ok(())
    }
}

impl Deserializable for ShardIdentFull {
    fn read_from(&mut self, cell: &mut SliceData) -> Result<()> {
        self.workchain_id.read_from(cell)?;
        self.prefix.read_from(cell)?;
        Ok(())
    }
}

impl fmt::Display for ShardIdentFull {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{:016X}", self.workchain_id, self.prefix)
    }
}

impl fmt::LowerHex for ShardIdentFull {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{:016X}", self.workchain_id, self.prefix)
    }
}

impl ShardHashes {
    pub fn iterate_shards_for_workchain<F>(&self, workchain_id: i32, mut func: F) -> Result<()>
    where F: FnMut(ShardIdent, ShardDescr) -> Result<bool> {
        if let Some(InRefValue(shards)) = self.get(&workchain_id)? {
            shards.iterate(|prefix, shard_descr| {
                let shard_ident = ShardIdent::with_prefix_slice(workchain_id, prefix)?;
                func(shard_ident, shard_descr)
            })?;
        }
        Ok(())
    }
    pub fn iterate_shards<F>(&self, mut func: F) -> Result<bool>
    where F: FnMut(ShardIdent, ShardDescr) -> Result<bool> {
        self.iterate_with_keys(|wc_id: i32, InRefValue(shards)| {
            shards.iterate(|prefix, shard_descr| {
                let shard_ident = ShardIdent::with_prefix_slice(wc_id, prefix)?;
                func(shard_ident, shard_descr)
            })
        })
    }
    pub fn iterate_shards_with_siblings<F>(&self, mut func: F) -> Result<bool>
    where F: FnMut(ShardIdent, ShardDescr, Option<ShardDescr>) -> Result<bool> {
        self.iterate_with_keys(|wc_id: i32, InRefValue(shards)| {
            shards.iterate_pairs(|prefix, shard_descr, sibling| {
                let prefix = SliceData::load_bitstring(prefix)?;
                let shard_ident = ShardIdent::with_prefix_slice(wc_id, prefix)?;
                func(shard_ident, shard_descr, sibling)
            })
        })
    }
    pub fn iterate_shards_with_siblings_mut<F>(&self, mut _func: F) -> Result<()>
    where F: FnMut(ShardIdent, ShardDescr, Option<ShardDescr>) -> Result<Option<ShardDescr>> {
        unimplemented!()
    }
    pub fn has_workchain(&self, workchain_id: i32) -> Result<bool> {
        self.get_as_slice(&workchain_id).map(|result| result.is_some())
    }
    pub fn find_shard(&self, shard: &ShardIdent) -> Result<Option<McShardRecord>> {
        if let Some(InRefValue(bintree)) = self.get(&shard.workchain_id())? {
            let shard_id = shard.shard_key(false);
            if let Some((key, descr)) = bintree.find(shard_id)? {
                let shard = ShardIdent::with_prefix_slice(shard.workchain_id(), key)?;
                return Ok(Some(McShardRecord::from_shard_descr(shard, descr)))
            }
        }
        Ok(None)
    }
    pub fn find_shard_by_prefix(&self, prefix: &AccountIdPrefixFull) -> Result<Option<McShardRecord>> {
        if let Some(InRefValue(bintree)) = self.get(&prefix.workchain_id())? {
            let shard_id = prefix.shard_key(false);
            if let Some((key, descr)) = bintree.find(shard_id)? {
                let shard = ShardIdent::with_prefix_slice(prefix.workchain_id(), key)?;
                return Ok(Some(McShardRecord::from_shard_descr(shard, descr)))
            }
        }
        Ok(None)
    }
    pub fn get_shard(&self, shard: &ShardIdent) -> Result<Option<McShardRecord>> {
        if let Some(InRefValue(bintree)) = self.get(&shard.workchain_id())? {
            let shard_id = shard.shard_key(false);
            if let Some(descr) = bintree.get(shard_id)? {
                return Ok(Some(McShardRecord::from_shard_descr(shard.clone(), descr)))
            }
        }
        Ok(None)
    }
    pub fn get_neighbours(&self, shard: &ShardIdent) -> Result<Vec<McShardRecord>> {
        let mut vec = Vec::new();
        self.iterate_with_keys(|workchain_id: i32, InRefValue(bintree)| {
            bintree.iterate(|prefix, shard_descr| {
                let shard_ident = ShardIdent::with_prefix_slice(workchain_id, prefix)?;
                if shard.is_neighbor_for(&shard_ident) {
                    vec.push(McShardRecord::from_shard_descr(shard_ident, shard_descr));
                }
                Ok(true)
            })?;
            Ok(true)
        })?;
        Ok(vec)
    }
    pub fn get_new_shards(&self) -> Result<HashMap<ShardIdent, Vec<BlockIdExt>>> {
        let mut new_shards = HashMap::new();
        self.iterate_shards(|shard, descr| {
            let block_id = BlockIdExt {
                shard_id: shard.clone(),
                seq_no: descr.seq_no,
                root_hash: descr.root_hash,
                file_hash: descr.file_hash,
            };
            if descr.before_split {
                let (l,r) = shard.split()?;
                new_shards.insert(l, vec![block_id.clone()]);
                new_shards.insert(r, vec![block_id]);
            } else if descr.before_merge {
                let p = shard.merge()?;
                new_shards.entry(p).or_insert_with(Vec::new).push(block_id)
            } else {
                new_shards.insert(shard, vec![block_id]);
            }
            Ok(true)
        })?;
        Ok(new_shards)
    }
    pub fn calc_shard_cc_seqno(&self, shard: &ShardIdent) -> Result<u32> {
        if shard.is_masterchain() {
            fail!("Given `shard` can't be masterchain")
        }
        ShardIdent::check_workchain_id(shard.workchain_id())?;

        let shard1 = self.find_shard(&shard.left_ancestor_mask()?)?
            .ok_or_else(|| error!("get_shard_cc_seqno: can't find shard1"))?;

        if shard1.shard().is_ancestor_for(shard) {
            return Ok(shard1.descr.next_catchain_seqno)
        } else if !shard.is_parent_for(shard1.shard()) {
            fail!("get_shard_cc_seqno: invalid shard1 {} for {}", shard1.shard(), shard)
        }

        let shard2 = self.find_shard(&shard.right_ancestor_mask()?)?
            .ok_or_else(|| error!("get_shard_cc_seqno: can't find shard2"))?;

        if !shard.is_parent_for(shard2.shard()) {
            fail!("get_shard_cc_seqno: invalid shard2 {} for {}", shard2.shard(), shard)
        }

        Ok(std::cmp::max(shard1.descr.next_catchain_seqno, shard2.descr.next_catchain_seqno) + 1)
    }
    pub fn split_shard(
        &mut self,
        splitted_shard: &ShardIdent,
        splitter: impl FnOnce(ShardDescr) -> Result<(ShardDescr, ShardDescr)>
    ) -> Result<()> {
        let mut tree = self.get(&splitted_shard.workchain_id())?
            .ok_or_else(|| error!("Can't find workchain {}", splitted_shard.workchain_id()))?;
        if !tree.0.split(splitted_shard.shard_key(false), splitter)? {
            fail!("Splitted shard {} is not found", splitted_shard)
        } else {
            self.set(&splitted_shard.workchain_id(), &tree)
        }
    }
    pub fn merge_shards(
        &mut self,
        new_shard: &ShardIdent,
        merger: impl FnOnce(ShardDescr, ShardDescr) -> Result<ShardDescr>
    ) -> Result<()> {
        let mut tree = self.get(&new_shard.workchain_id())?
            .ok_or_else(|| error!("Can't find workchain {}", new_shard.workchain_id()))?;
        if !tree.0.merge(new_shard.shard_key(false), merger)? {
            fail!("Merged shards's parent {} is not found", new_shard)
        } else {
            self.set(&new_shard.workchain_id(), &tree)
        }
    }
    pub fn update_shard(
        &mut self,
        shard: &ShardIdent,
        mutator: impl FnOnce(ShardDescr) -> Result<ShardDescr>
    ) -> Result<()> {
        let mut tree = self.get(&shard.workchain_id())?
            .ok_or_else(|| error!("Can't find workchain {}", shard.workchain_id()))?;
        if !tree.0.update(shard.shard_key(false), mutator)? {
            fail!("Updated shard {} is not found", shard)
        } else {
            self.set(&shard.workchain_id(), &tree)
        }
    }
    pub fn add_workchain(
        &mut self,
        workchain_id: i32,
        reg_mc_seqno: u32,
        zerostate_root_hash: UInt256,
        zerostate_file_hash: UInt256,
        collators: Option<ShardCollators>,
    ) -> Result<()> {

        if self.has_workchain(workchain_id)? {
            fail!("Workchain {} is already added", workchain_id);
        }

        let descr = ShardDescr {
            reg_mc_seqno,
            root_hash: zerostate_root_hash,
            file_hash: zerostate_file_hash,
            next_validator_shard: SHARD_FULL,
            collators,
            ..ShardDescr::default()
        };
        let tree = BinTree::with_item(&descr)?;

        self.set(&workchain_id, &InRefValue(tree))
    }
}

impl ShardHashes {
    pub fn dump(&self, heading: &str) -> usize {
        let mut count = 0;
        println!("dumping shard records for: {}", heading);
        self.iterate_with_keys(|workchain_id: i32, InRefValue(bintree)| {
            println!("workchain: {}", workchain_id);
            bintree.iterate(|prefix, descr| {
                let shard = ShardIdent::with_prefix_slice(workchain_id, prefix)?;
                println!(
                    "shard: {:064b} seq_no: {} shard: 0x{}",
                    shard.shard_prefix_with_tag(),
                    descr.seq_no,
                    shard.shard_prefix_as_str_with_tag()
                );
                count += 1;
                Ok(true)
            })
        }).unwrap();
        count
    }
}

#[derive(Clone, Default, Debug, Eq, PartialEq)]
pub struct McShardRecord {
    pub descr: ShardDescr,
    pub block_id: BlockIdExt,
}

impl McShardRecord {
    pub fn from_shard_descr(shard: ShardIdent, descr: ShardDescr) -> Self {
        let block_id = BlockIdExt::with_params(shard, descr.seq_no, descr.root_hash.clone(), descr.file_hash.clone());
        Self { descr, block_id }
    }

    pub fn from_block(block: &Block, block_id: BlockIdExt) -> Result<Self> {
        let info = block.read_info()?;
        let value_flow = block.read_value_flow()?;
        Ok(
            McShardRecord {
                descr: ShardDescr {
                    seq_no: info.seq_no(),
                    reg_mc_seqno: 0xffff_ffff, // by t-node
                    start_lt: info.start_lt(),
                    end_lt: info.end_lt(),
                    root_hash: block_id.root_hash().clone(),
                    file_hash: block_id.file_hash().clone(),
                    before_split: info.before_split(),
                    before_merge: false, // by t-node
                    want_split: info.want_split(),
                    want_merge: info.want_merge(),
                    nx_cc_updated: false, // by t-node
                    flags: info.flags() & !7,
                    next_catchain_seqno: info.gen_catchain_seqno(),
                    next_validator_shard: info.shard().shard_prefix_with_tag(),
                    min_ref_mc_seqno: info.min_ref_mc_seqno(),
                    gen_utime: info.gen_utime().into(),
                    split_merge_at: FutureSplitMerge::None, // is not used in McShardRecord
                    fees_collected: value_flow.fees_collected,
                    funds_created: value_flow.created,
                    copyleft_rewards: value_flow.copyleft_rewards,
                    pack_info: info.read_pack_info()?,
                    ..Default::default()
                },
                block_id,
            }
        )
    }

    pub fn from_block_and_proof_chain(
        block: &Block,
        block_id: BlockIdExt,
        proof_chain: ProofChain
    ) -> Result<Self> {
        let mut record = Self::from_block(block, block_id)?;
        record.descr.proof_chain = Some(proof_chain);
        Ok(record)
    }

    pub fn shard(&self) -> &ShardIdent { self.block_id.shard() }

    pub fn descr(&self) -> &ShardDescr { &self.descr }

    // to be deleted
    pub fn blk_id(&self) -> &BlockIdExt { &self.block_id }

    pub fn block_id(&self) -> &BlockIdExt { &self.block_id }

    pub fn basic_info_equal(&self, other: &Self, compare_fees: bool, compare_reg_seqno: bool) -> bool {
        self.block_id == other.block_id
            && self.descr.start_lt == other.descr.start_lt
            && self.descr.end_lt == other.descr.end_lt
            && (!compare_reg_seqno || self.descr.reg_mc_seqno == other.descr.reg_mc_seqno)
            && self.descr.gen_utime == other.descr.gen_utime
            && self.descr.min_ref_mc_seqno == other.descr.min_ref_mc_seqno
            && self.descr.before_split == other.descr.before_split
            && self.descr.want_split == other.descr.want_split
            && self.descr.want_merge == other.descr.want_merge
            && (!compare_fees
                || (self.descr.fees_collected == other.descr.fees_collected
                    && self.descr.funds_created == other.descr.funds_created
                    && self.descr.copyleft_rewards == other.descr.copyleft_rewards))
    }
}

impl ShardFees {
    pub fn store_shard_fees(
        &mut self,
        shard: &ShardIdent,
        fees: CurrencyCollection,
        created: CurrencyCollection
    ) -> Result<()> {
        let id = ShardIdentFull{
            workchain_id: shard.workchain_id(),
            prefix: shard.shard_prefix_with_tag(),
        };
        let fee = ShardFeeCreated{fees, create: created};
        self.set(&id, &fee, &fee)?;
        Ok(())
    }
}

define_HashmapE!{CopyleftMessages, 15, InRefValue<InMsg>}

/*
masterchain_block_extra#cca5
  key_block:(## 1)
  shard_hashes:ShardHashes
  shard_fees:ShardFees
  ^[ prev_blk_signatures:(HashmapE 16 CryptoSignaturePair)
     recover_create_msg:(Maybe ^InMsg)
     mint_msg:(Maybe ^InMsg) ]
  config:key_block?ConfigParams
= McBlockExtra;
*/
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct McBlockExtra {
    key_block: bool,
    shards: ShardHashes, // workchain_id of ShardIdent from all blocks
    fees: ShardFees,
    prev_blk_signatures: CryptoSignatures,
    recover_create_msg: Option<ChildCell<InMsg>>,
    copyleft_msgs: CopyleftMessages,
    mint_msg: Option<ChildCell<InMsg>>,
    mesh: MeshHashesExt,
    config: Option<ConfigParams>,
    validators_stat: ValidatorsStat,
    serde_opts: u8,
}

impl McBlockExtra {
    pub fn with_common_message_support() -> Self {
        let serde_opts = SERDE_OPTS_COMMON_MESSAGE;
        Self {
            serde_opts,
            copyleft_msgs: CopyleftMessages::with_serde_opts(serde_opts),
            ..Default::default()
        }
    }
    ///
    /// Get all fees for blockchain
    ///
    pub fn total_fee(&self) -> &CurrencyCollection {
        &self.fees.root_extra().fees
    }


    ///
    /// Get total fees for shard
    ///
    pub fn fee(&self, ident: &ShardIdent) -> Result<Option<CurrencyCollection>> {
        Ok(match self.fees.get_serialized(ident.full_key()?)? {
            Some(shards) => Some(shards.fees),
            None => None
        })
    }

    pub fn is_key_block(&self) -> bool { self.config.is_some() }

    pub fn hashes(&self) -> &ShardHashes { &self.shards }
    pub fn hashes_mut(&mut self) -> &mut ShardHashes { &mut self.shards }

    pub fn shards(&self) -> &ShardHashes { &self.shards }
    pub fn shards_mut(&mut self) -> &mut ShardHashes { &mut self.shards }

    pub fn fees(&self) -> &ShardFees { &self.fees }
    pub fn fees_mut(&mut self) -> &mut ShardFees { &mut self.fees }

    pub fn prev_blk_signatures(&self) -> &CryptoSignatures { &self.prev_blk_signatures }
    pub fn prev_blk_signatures_mut(&mut self) -> &mut CryptoSignatures { &mut self.prev_blk_signatures }

    pub fn config(&self) -> Option<&ConfigParams> { self.config.as_ref() }
    pub fn config_mut(&mut self) -> &mut Option<ConfigParams> { &mut self.config }
    pub fn set_config(&mut self, config: ConfigParams) { self.config = Some(config) }

    pub fn read_recover_create_msg(&self) -> Result<Option<InMsg>> {
        self.recover_create_msg.as_ref().map(|mr| mr.read_struct()).transpose()
    }
    pub fn write_recover_create_msg(&mut self, value: Option<&InMsg>) -> Result<()> {
        self.recover_create_msg = value
            .map(|v| ChildCell::with_struct_and_opts(v, self.serde_opts))
            .transpose()?;
        Ok(())
    }
    pub fn recover_create_msg_cell(&self) -> Option<Cell> {
        self.recover_create_msg.as_ref().map(|mr| mr.cell())
    }

    pub fn read_mint_msg(&self) -> Result<Option<InMsg>> {
        self.mint_msg.as_ref().map(ChildCell::read_struct).transpose()
    }
    pub fn write_mint_msg(&mut self, value: Option<&InMsg>) -> Result<()> {
        self.mint_msg = value.map(|v| ChildCell::with_struct_and_opts(v, self.serde_opts)).transpose()?;
        Ok(())
    }
    pub fn mint_msg_cell(&self) -> Option<Cell> {
        self.mint_msg.as_ref().map(|mr| mr.cell())
    }

    pub fn read_copyleft_msgs(&self) -> Result<Vec<InMsg>> {
        let mut result = Vec::<InMsg>::default();
        for i in 0..self.copyleft_msgs.len()? {
            result.push(self.copyleft_msgs.get(&U15(i as i16))?.ok_or_else(|| error!("Cant find index {} in map", i))?.inner());
        }
        Ok(result)
    }
    pub fn write_copyleft_msgs(&mut self, value: &[InMsg]) -> Result<()> {
        for (i, rec) in value.iter().enumerate() {
            self.copyleft_msgs.setref(&U15(i as i16), &rec.serialize_with_opts(self.serde_opts)?)?;
        }
        Ok(())
    }

    pub fn mesh_descr(&self) -> &MeshHashesExt {
        &self.mesh
    }
    pub fn mesh_descr_mut(&mut self) -> &mut MeshHashesExt {
        &mut self.mesh
    }
    pub fn serde_opts(&self) -> u8 {
        self.serde_opts
    }

    pub fn validators_stat(&self) -> &ValidatorsStat {
        &self.validators_stat
    }

    pub fn validators_stat_mut(&mut self) -> &mut ValidatorsStat {
        &mut self.validators_stat
    }

    pub fn set_validators_stat(&mut self, stat: ValidatorsStat) {
        self.validators_stat = stat;
    }
}

const MC_BLOCK_EXTRA_TAG : u16 = 0xCCA5;   // Original struct.
const MC_BLOCK_EXTRA_TAG_2 : u16 = 0xdc75; // With copyleft, but without common messages and mesh.
const MC_BLOCK_EXTRA_TAG_3 : u16 = 0xdc76; // With common messages and mesh (might be empty),
                                           // but without copyleft!

impl Deserializable for McBlockExtra {
    fn read_from(&mut self, cell: &mut SliceData) -> Result<()> {
        let tag = cell.get_next_u16()?;
        if tag != MC_BLOCK_EXTRA_TAG && tag != MC_BLOCK_EXTRA_TAG_2 && tag != MC_BLOCK_EXTRA_TAG_3 {
            fail!(
                BlockError::InvalidConstructorTag {
                    t: tag.into(),
                    s: std::any::type_name::<Self>().to_string()
                }
            )
        }
        self.serde_opts = match tag {
            MC_BLOCK_EXTRA_TAG_3 => SERDE_OPTS_COMMON_MESSAGE,
            _ => 0,
        };
        let key_block = cell.get_next_bit()?;
        self.shards.read_from(cell)?;
        self.fees.read_from(cell)?;

        let cell1 = &mut SliceData::load_cell(cell.checked_drain_reference()?)?;
        self.prev_blk_signatures.read_from(cell1)?;
        self.recover_create_msg.read_from_with_opts(cell1, self.serde_opts)?;
        self.mint_msg.read_from_with_opts(cell1, self.serde_opts)?;

        if tag == MC_BLOCK_EXTRA_TAG_2 {
            self.copyleft_msgs.read_from(cell1)?;
        } else if tag == MC_BLOCK_EXTRA_TAG_3 {
            self.mesh.read_from(cell1)?;
            self.copyleft_msgs = CopyleftMessages::with_serde_opts(self.serde_opts);
        }

        self.config = if key_block {
            Some(ConfigParams::construct_from(cell)?)
        } else {
            None
        };

        Ok(())
    }
}

impl Serializable for McBlockExtra {
    fn write_to(&self, cell: &mut BuilderData) -> Result<()> {
        self.write_with_opts(cell, SERDE_OPTS_EMPTY)
    }
    fn write_with_opts(&self, cell: &mut BuilderData, opts: u8) -> Result<()> {
        let copyleft = !self.copyleft_msgs.is_empty();
        let common_message = opts & SERDE_OPTS_COMMON_MESSAGE != 0;
        if copyleft && common_message {
            fail!("copyleft and common messages is not supported together");
        }
        if !self.mesh.is_empty() && !common_message {
            fail!("mesh is not empty but common messages option is not set");
        }
        let tag = if copyleft {
            MC_BLOCK_EXTRA_TAG_2
        } else if common_message {
            MC_BLOCK_EXTRA_TAG_3
        } else {
            MC_BLOCK_EXTRA_TAG
        };
        cell.append_u16(tag)?;
        self.config.is_some().write_to(cell)?;
        self.shards.write_to(cell)?;
        self.fees.write_to(cell)?;

        let mut cell1 = self.prev_blk_signatures.write_to_new_cell()?;
        self.recover_create_msg.write_to(&mut cell1)?;
        self.mint_msg.write_to(&mut cell1)?;

        if copyleft {
            self.copyleft_msgs.write_to(&mut cell1)?;
        } else if common_message {
            self.mesh.write_to(&mut cell1)?;
        }

        cell.checked_append_reference(cell1.into_cell()?)?;

        if let Some(config) = &self.config {
            config.write_to(cell)?;
        }

        Ok(())
    }
}

// _ key:Bool max_end_lt:uint64 = KeyMaxLt;
#[derive(Default, Clone, Debug, Eq, PartialEq)]
pub struct KeyMaxLt {
    pub key: bool,
    pub max_end_lt: u64
}

impl Deserializable for KeyMaxLt {
    fn read_from(&mut self, slice: &mut SliceData) -> Result<()> {
        self.key.read_from(slice)?;
        self.max_end_lt.read_from(slice)?;
        Ok(())
    }
}

impl Serializable for KeyMaxLt {
    fn write_to(&self, cell: &mut BuilderData) -> Result<()> {
        self.key.write_to(cell)?;
        self.max_end_lt.write_to(cell)?;
        Ok(())
    }
}

impl Augmentable for KeyMaxLt {
    fn calc(&mut self, other: &Self) -> Result<bool> {
        if other.key {
            self.key = true
        }
        if self.max_end_lt < other.max_end_lt {
            self.max_end_lt = other.max_end_lt
        }
        Ok(true)
    }
}

// _ key:Bool blk_ref:ExtBlkRef = KeyExtBlkRef;
#[derive(Default, Clone, Debug, Eq, PartialEq)]
pub struct KeyExtBlkRef {
    pub key: bool,
    pub blk_ref: ExtBlkRef
}

impl KeyExtBlkRef {
    pub fn key(&self) -> bool {
        self.key
    }
    pub fn blk_ref(&self) -> &ExtBlkRef {
        &self.blk_ref
    }
    pub fn master_block_id(self) -> (u64, BlockIdExt, bool) {
        (self.blk_ref.end_lt, BlockIdExt::from_ext_blk(self.blk_ref), self.key)
    }
}

impl Deserializable for KeyExtBlkRef {
    fn read_from(&mut self, slice: &mut SliceData) -> Result<()> {
        self.key.read_from(slice)?;
        self.blk_ref.read_from(slice)?;
        Ok(())
    }
}

impl Serializable for KeyExtBlkRef {
    fn write_to(&self, cell: &mut BuilderData) -> Result<()> {
        self.key.write_to(cell)?;
        self.blk_ref.write_to(cell)?;
        Ok(())
    }
}

impl Augmentation<KeyMaxLt> for KeyExtBlkRef {
    fn aug(&self) -> Result<KeyMaxLt> {
        Ok(KeyMaxLt {
            key: self.key,
            max_end_lt: self.blk_ref.end_lt
        })
    }
}

// _ (HashmapAugE 32 KeyExtBlkRef KeyMaxLt) = OldMcBlocksInfo;
// key - seq_no
define_HashmapAugE!(OldMcBlocksInfo, 32, u32, KeyExtBlkRef, KeyMaxLt);

impl OldMcBlocksInfo {

    // returns key block with max block.seqno and block.seqno <= req_seqno
    pub fn get_prev_key_block(&self, req_seqno: u32) -> Result<Option<ExtBlkRef>> {
        let found = self.traverse(|key_prefix, key_prefix_len, aug, value_opt| {
            if !aug.key {
                // no key blocks in subtree, skip
                return Ok(TraverseNextStep::Stop);
            }

            let x = Self::build_key_part(key_prefix, key_prefix_len)?;
            let d = 32 - key_prefix_len;
            if d == 0 {
                return if x <= req_seqno {
                    let value = value_opt.ok_or_else(|| error!(BlockError::InvalidData(
                        "OldMcBlocksInfo's node with max key length doesn't have value".to_string()
                    )))?;
                    Ok(TraverseNextStep::End(value))
                } else {
                    Ok(TraverseNextStep::Stop)
                }
            }
            let y = req_seqno >> (d - 1);
            match y.cmp(&(2 * x)) {
                std::cmp::Ordering::Less => {
                    // (x << d) > req_seqno <=> x > (req_seqno >> d) = (y >> 1) <=> 2 * x > y
                    Ok(TraverseNextStep::Stop) // all nodes in subtree have block.seqno > req_seqno => skip
                }
                std::cmp::Ordering::Equal => {
                    Ok(TraverseNextStep::VisitZero) // visit only left ("0")
                }
                _ => {
                    Ok(TraverseNextStep::VisitOneZero) // visit right, then left ("1" then "0")
                }
            }
        })?;

        if let Some(id) = found {
            debug_assert!(id.blk_ref.seq_no <= req_seqno);
            debug_assert!(id.key);
            Ok(Some(id.blk_ref))
        } else {
            Ok(None)
        }
    }

    // returns key block with min block.seqno and block.seqno >= req_seqno
    pub fn get_next_key_block(&self, req_seqno: u32) -> Result<Option<ExtBlkRef>> {
        let found = self.traverse(|key_prefix, key_prefix_len, aug, value_opt| {
            if !aug.key {
                // no key blocks in subtree, skip
                return Ok(TraverseNextStep::Stop);
            }

            let x = Self::build_key_part(key_prefix, key_prefix_len)?;
            let d = 32 - key_prefix_len;
            if d == 0 {
                return if x >= req_seqno {
                    let value = value_opt.ok_or_else(|| error!(BlockError::InvalidData(
                        "OldMcBlocksInfo's node with max key length doesn't have value".to_string()
                    )))?;
                    Ok(TraverseNextStep::End(value))
                } else {
                    Ok(TraverseNextStep::Stop)
                }
            }
            let y = req_seqno >> (d - 1);
            match y.cmp(&(2 * x + 1)) {
                std::cmp::Ordering::Greater => {
                    // ((x + 1) << d) <= req_seqno <=> (x+1) <= (req_seqno >> d) = (y >> 1) <=> 2*x+2 <= y <=> y > 2*x+1
                    Ok(TraverseNextStep::Stop) // all nodes in subtree have block.seqno < req_seqno => skip
                }
                std::cmp::Ordering::Equal => {
                    Ok(TraverseNextStep::VisitOne) // visit only right ("1")
                }
                _ => {
                    Ok(TraverseNextStep::VisitZeroOne) // visit left, then right ("0" then "1")
                }
            }
        })?;

        if let Some(id) = found {
            debug_assert!(id.blk_ref.seq_no >= req_seqno);
            debug_assert!(id.key);
            Ok(Some(id.blk_ref))
        } else {
            Ok(None)
        }
    }

    pub fn check_block(&self, id: &BlockIdExt) -> Result<()> {
        self.check_key_block(id, None)
    }

    pub fn check_key_block(&self, id: &BlockIdExt, is_key_opt: Option<bool>) -> Result<()> {
        if !id.shard().is_masterchain() {
            fail!(BlockError::InvalidData("Given id does not belong masterchain".to_string()));
        }
        let found_id = self
            .get(&id.seq_no())?
            .ok_or_else(|| error!("Block with given seq_no {} is not found", id.seq_no()))?;

        if found_id.blk_ref.root_hash != *id.root_hash() {
            fail!("Given block has invalid root hash: found {:x}, expected {:x}",
                found_id.blk_ref.root_hash, id.root_hash())
        }
        if found_id.blk_ref.file_hash != *id.file_hash() {
            fail!("Given block has invalid file hash: found {:x}, expected {:x}",
                found_id.blk_ref.file_hash, id.file_hash())
        }
        if let Some(is_key) = is_key_opt {
            if is_key != found_id.key {
                fail!(
                    "Given block has key flag set to: {}, expected {}",
                    found_id.key, is_key
                )
            }
        }
        Ok(())
    }

    fn build_key_part(key_prefix: &[u8], key_prefix_len: usize) -> Result<u32> {
        if key_prefix_len > 32 {
            fail!(BlockError::InvalidData("key_prefix_len > 32".to_string()));
        }
        let mut key_buf = [0_u8; 4];
        key_buf[..key_prefix.len()].copy_from_slice(key_prefix);
        Ok(
            u32::from_be_bytes(key_buf) >> (32 - key_prefix_len)
        )
    }
}

// _ fees:CurrencyCollection create:CurrencyCollection = ShardFeeCreated;
#[derive(Default, Clone, Debug, Eq, PartialEq)]
pub struct ShardFeeCreated {
    pub fees: CurrencyCollection,
    pub create: CurrencyCollection,
}

impl ShardFeeCreated {
    pub fn with_fee(fees: CurrencyCollection) -> Self {
        Self {
            fees,
            create: CurrencyCollection::default(),
        }
    }
}

impl Augmentable for ShardFeeCreated {
    fn calc(&mut self, other: &Self) -> Result<bool> {
        let mut result = self.fees.calc(&other.fees)?;
        result |= self.create.calc(&other.create)?;
        Ok(result)
    }
}

impl Deserializable for ShardFeeCreated {
    fn read_from(&mut self, cell: &mut SliceData) -> Result<()> {
        self.fees.read_from(cell)?;
        self.create.read_from(cell)?;
        Ok(())
    }
}

impl Serializable for ShardFeeCreated {
    fn write_to(&self, cell: &mut BuilderData) -> Result<()> {
        self.fees.write_to(cell)?;
        self.create.write_to(cell)?;
        Ok(())
    }
}

pub fn umulnexps32(x : u64, k : u32, _trunc : bool) -> u64 {
    (
        (x as f64 * (k as f64 / -65536f64).exp()) // x * exp(-k / 2^16)
        + 0.5f64 // Need to round up the number to the nearest integer
    ) as u64
}

/// counters#_ last_updated:uint32 total:uint64 cnt2048:uint64 cnt65536:uint64 = Counters;
#[derive(Clone, Debug, Default, Eq)]
pub struct Counters {
    last_updated: u32,
    total: u64,
    cnt2048: u64,
    cnt65536: u64,
}

impl PartialEq for Counters {
    fn eq(&self, other: &Self) -> bool {
        self.last_updated == other.last_updated
        && self.total == other.total
        && self.cnt2048 == other.cnt2048
        && self.cnt65536 == other.cnt65536
    }
}

impl Counters {
    pub fn is_valid(&self) -> bool {
        if self.total == 0 {
            if (self.cnt2048 | self.cnt65536) != 0 {
                return false;
            }
        } else if self.last_updated == 0 {
            return false;
        }
        true
    }
    pub fn is_zero(&self) -> bool {
        self.total == 0
    }
    pub fn almost_zero(&self) -> bool {
        (self.cnt2048 | self.cnt65536) <= 1
    }
    pub fn almost_equals(&self, other: &Self) -> bool {
        self.last_updated == other.last_updated
            && self.total == other.total
            && self.cnt2048 <= other.cnt2048 + 1
            && other.cnt2048 <= self.cnt2048 + 1
            && self.cnt65536 <= other.cnt65536 + 1
            && other.cnt65536 <= self.cnt65536 + 1
    }
    pub fn modified_since(&self, utime: u32) -> bool {
        self.last_updated >= utime
    }
    pub fn increase_by(&mut self, count: u64, now: u32) -> bool {
        if !self.is_valid() {
            return false
        }
        let scaled = count << 32;
        if self.total == 0 {
            self.last_updated = now;
            self.total = count;
            self.cnt2048 = scaled;
            self.cnt65536 = scaled;
            return true
        }
        if count > !self.total || self.cnt2048 > !scaled || self.cnt65536 > !scaled {
            return false;
        }
        let dt = now.checked_sub(self.last_updated).unwrap_or_default();
        if dt != 0 {
            // more precise version of cnt2048 = llround(cnt2048 * exp(-dt / 2048.));
            // (rounding error has absolute value < 1)
            self.cnt2048 = if dt >= 48 * 2048 {0} else {
                umulnexps32(self.cnt2048, dt << 5, false)
            };
            // more precise version of cnt65536 = llround(cnt65536 * exp(-dt / 65536.));
            // (rounding error has absolute value < 1)
            self.cnt65536 = umulnexps32(self.cnt65536, dt, false);
        }
        self.total += count;
        self.cnt2048 += scaled;
        self.cnt65536 += scaled;
        self.last_updated = now;
        true
    }
    pub fn total(&self) -> u64 {
        self.total
    }
    pub fn last_updated(&self) -> u32 {
        self.last_updated
    }
    pub fn cnt2048(&self) -> u64 {
        self.cnt2048
    }
    pub fn cnt65536(&self) -> u64 {
        self.cnt65536
    }
}

impl Deserializable for Counters {
    fn read_from(&mut self, slice: &mut SliceData) -> Result<()> {
        self.last_updated.read_from(slice)?;
        self.total.read_from(slice)?;
        self.cnt2048.read_from(slice)?;
        self.cnt65536.read_from(slice)?;
        Ok(())
    }
}

impl Serializable for Counters {
    fn write_to(&self, cell: &mut BuilderData) -> Result<()> {
        self.last_updated.write_to(cell)?;
        self.total.write_to(cell)?;
        self.cnt2048.write_to(cell)?;
        self.cnt65536.write_to(cell)?;
        Ok(())
    }
}

/// creator_info#4 mc_blocks:Counters shard_blocks:Counters = CreatorStats;
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CreatorStats {
    pub mc_blocks: Counters,
    pub shard_blocks: Counters,
}

impl CreatorStats {
    pub fn tag() -> u32 {
        0x4
    }

    pub fn tag_len_bits() -> usize {
        4
    }

    pub fn mc_blocks(&self) -> &Counters {
        &self.mc_blocks
    }

    pub fn shard_blocks(&self) -> &Counters {
        &self.shard_blocks
    }
}

impl Deserializable for CreatorStats {
    fn read_from(&mut self, slice: &mut SliceData) -> Result<()> {
        let tag = slice.get_next_int(Self::tag_len_bits())? as u32;
        if tag != Self::tag() {
            fail!(
                BlockError::InvalidConstructorTag {
                    t: tag,
                    s: std::any::type_name::<Self>().to_string()
                }
            )
        }

        self.mc_blocks.read_from(slice)?;
        self.shard_blocks.read_from(slice)?;
        Ok(())
    }
}

impl Serializable for CreatorStats {
    fn write_to(&self, cell: &mut BuilderData) -> Result<()> {
        cell.append_bits(Self::tag() as usize, Self::tag_len_bits())?;

        self.mc_blocks.write_to(cell)?;
        self.shard_blocks.write_to(cell)?;
        Ok(())
    }
}

define_HashmapE!{BlockCounters, 256, CreatorStats}

/// block_create_stats#17 counters:(HashmapE 256 CreatorStats) = BlockCreateStats;
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BlockCreateStats {
    pub counters: BlockCounters,
}

impl BlockCreateStats {
    pub fn tag() -> u32 {
        0x17
    }

    pub fn tag_len_bits() -> usize {
        8
    }
}

impl Deserializable for BlockCreateStats {
    fn read_from(&mut self, slice: &mut SliceData) -> Result<()> {
        let tag = slice.get_next_int(Self::tag_len_bits())? as u32;
        if tag != Self::tag() {
            fail!(
                BlockError::InvalidConstructorTag {
                    t: tag,
                    s: std::any::type_name::<Self>().to_string()
                }
            )
        }

        self.counters.read_from(slice)?;
        Ok(())
    }
}

impl Serializable for BlockCreateStats {
    fn write_to(&self, cell: &mut BuilderData) -> Result<()> {
        cell.append_bits(Self::tag() as usize, Self::tag_len_bits())?;

        self.counters.write_to(cell)?;
        Ok(())
    }
}

define_HashmapE!{MeshHashes, 32, ConnectedNwDescr}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConnectedNwDescr {
    pub seq_no: u32,
    pub root_hash: UInt256,
    pub file_hash: UInt256,
    pub imported: VarUInteger32,
    pub gen_utime: u32,

}

const CONNECTED_NW_DESCR_TAG: u8 = 0x01;

impl Deserializable for ConnectedNwDescr {
    fn read_from(&mut self, slice: &mut SliceData) -> Result<()> {
        let tag = slice.get_next_byte()?;
        if tag != CONNECTED_NW_DESCR_TAG {
            fail!(
                BlockError::InvalidConstructorTag {
                    t: tag.into(),
                    s: std::any::type_name::<Self>().to_string()
                }
            )
        }
        self.seq_no.read_from(slice)?;
        self.root_hash.read_from(slice)?;
        self.file_hash.read_from(slice)?;
        self.imported.read_from(slice)?;
        self.gen_utime.read_from(slice)?;
        Ok(())
    }
}

impl Serializable for ConnectedNwDescr {
    fn write_to(&self, cell: &mut BuilderData) -> Result<()> {
        cell.append_u8(CONNECTED_NW_DESCR_TAG)?;
        self.seq_no.write_to(cell)?;
        self.root_hash.write_to(cell)?;
        self.file_hash.write_to(cell)?;
        self.imported.write_to(cell)?;
        self.gen_utime.write_to(cell)?;
        Ok(())
    }
}

/*
masterchain_state_extra#cc26
  shard_hashes:ShardHashes
  config:ConfigParams
  ^[ flags:(## 16) { flags <= 1 }
     validator_info:ValidatorInfo
     prev_blocks:OldMcBlocksInfo
     after_key_block:Bool
     last_key_block:(Maybe ExtBlkRef)
     block_create_stats:(flags . 0)?BlockCreateStats ]
  global_balance:CurrencyCollection
= McStateExtra;
*/
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct McStateExtra {
    pub shards: ShardHashes,
    pub mesh: MeshHashes,
    pub config: ConfigParams,
    pub validator_info: ValidatorInfo,
    pub prev_blocks: OldMcBlocksInfo,
    pub after_key_block: bool,
    pub last_key_block: Option<ExtBlkRef>,
    pub block_create_stats: Option<BlockCreateStats>,
    pub global_balance: CurrencyCollection,
    pub state_copyleft_rewards: CopyleftRewards,
    pub validators_stat: ValidatorsStat,
}

const MC_STATE_EXTRA_TAG: u16 = 0xcc26;
const MC_STATE_CREATE_STATS_FLAG: u16 = 0b0001;
const MC_STATE_COPYLEFT_FLAG: u16 = 0b0010;
const MC_STATE_MESH_FLAG: u16 = 0b0100;
const MC_STATE_VAL_STAT_FLAG: u16 = 0b1000;

impl McStateExtra {

    /// Adds new workchain
    pub fn add_workchain(&mut self, workchain_id: i32, descr: &ShardDescr) -> Result<ShardIdent> {
        let shards = BinTree::with_item(descr)?;
        self.shards.set(&workchain_id, &InRefValue(shards))?;
        ShardIdent::with_workchain_id(workchain_id)
    }

    ///
    /// Get Shard last seq_no
    ///
    pub fn shard_seq_no(&self, ident: &ShardIdent) -> Result<Option<u32>> {
        Ok(match self.shards.get(&ident.workchain_id())? {
            Some(InRefValue(shards)) => shards.get(ident.shard_key(false))?.map(|s| s.seq_no),
            None => None
        })
    }

    ///
    /// Get shard last Logical Time
    ///
    pub fn shard_lt(&self, ident: &ShardIdent) -> Result<Option<u64>> {
        Ok(match self.shards.get(&ident.workchain_id())? {
            Some(InRefValue(shards)) => shards.get(ident.shard_key(false))?.map(|s| s.start_lt),
            None => None
        })
    }

    ///
    /// Get shard last block hash
    ///
    pub fn shard_hash(&self, ident: &ShardIdent) -> Result<Option<UInt256>> {
        Ok(match self.shards.get(&ident.workchain_id())? {
            Some(InRefValue(shards)) => shards.get(ident.shard_key(false))?.map(|s| s.root_hash),
            None => None
        })
    }

    pub fn shards(&self) -> &ShardHashes {
        &self.shards
    }
    pub fn config(&self) -> &ConfigParams {
        &self.config
    }
}

impl Deserializable for McStateExtra {
    fn read_from(&mut self, cell: &mut SliceData) -> Result<()> {
        let tag = cell.get_next_u16()?;
        if tag != MC_STATE_EXTRA_TAG {
            fail!(
                BlockError::InvalidConstructorTag {
                    t: tag.into(),
                    s: std::any::type_name::<Self>().to_string()
                }
            )
        }
        self.shards.read_from(cell)?;
        self.config.read_from(cell)?;

        let cell1 = &mut SliceData::load_cell(cell.checked_drain_reference()?)?;
        let mut flags = 0u16;
        flags.read_from(cell1)?; // 16 + 0
        if flags > 15 {
            fail!(
                BlockError::InvalidData(
                    format!("Invalid flags value ({}). Must be <= 7.", flags)
                )
            )
        }
        self.validator_info.read_from(cell1)?; // 65 + 0
        self.prev_blocks.read_from(cell1)?; // 1 + 1
        self.after_key_block.read_from(cell1)?; // 1 + 0
        self.last_key_block.read_from(cell1)?; // 609 + 0
        self.block_create_stats = if flags & MC_STATE_CREATE_STATS_FLAG == 0 {
            None
        } else {
            Some(BlockCreateStats::construct_from(cell1)?) // 1 + 1
        };
        let flag_copileft = flags & MC_STATE_COPYLEFT_FLAG != 0;
        let flag_val_stat = flags & MC_STATE_VAL_STAT_FLAG != 0;
        if flag_copileft && flag_val_stat {
            fail!("state_copyleft_rewards and validators_stats is not supported together");
        }
        if flag_copileft {
            self.state_copyleft_rewards.read_from(cell1)?; // 1 + 1
        }
        if flags & MC_STATE_MESH_FLAG != 0 {
            self.mesh.read_from(cell1)?;
        }
        if flag_val_stat {
            self.validators_stat.read_from(cell1)?;
        }
        self.global_balance.read_from(cell)?;
        Ok(())
    }
}

impl Serializable for McStateExtra {
    fn write_to(&self, builder: &mut BuilderData) -> Result<()> {
        if !self.state_copyleft_rewards.is_empty() && !self.validators_stat.is_empty() {
            fail!("state_copyleft_rewards and validators_stats is not supported together");
        }
        builder.append_u16(MC_STATE_EXTRA_TAG)?;
        self.shards.write_to(builder)?;
        self.config.write_to(builder)?;

        let mut builder1 = BuilderData::new();
        let mut flags = 0;
        if self.block_create_stats.is_some() {
            flags |= MC_STATE_CREATE_STATS_FLAG;
        }
        if !self.state_copyleft_rewards.is_empty() {
            flags |= MC_STATE_COPYLEFT_FLAG;
        }
        if !self.mesh.is_empty() {
            flags |= MC_STATE_MESH_FLAG;
        }
        if !self.validators_stat.is_empty() {
            flags |= MC_STATE_VAL_STAT_FLAG;
        }
        flags.write_to(&mut builder1)?;
        self.validator_info.write_to(&mut builder1)?;
        self.prev_blocks.write_to(&mut builder1)?;
        self.after_key_block.write_to(&mut builder1)?;
        self.last_key_block.write_to(&mut builder1)?;
        if let Some(ref block_create_stats) = self.block_create_stats {
            block_create_stats.write_to(&mut builder1)?;
        }
        if !self.state_copyleft_rewards.is_empty() {
            self.state_copyleft_rewards.write_to(&mut builder1)?;
        }
        if !self.mesh.is_empty() {
            self.mesh.write_to(&mut builder1)?;
        }
        if !self.validators_stat.is_empty() {
            self.validators_stat.write_to(&mut builder1)?;
        }
        builder.checked_append_reference(builder1.into_cell()?)?;

        self.global_balance.write_to(builder)?;
        Ok(())
    }
}

/*
fsm_none$0

fsm_split$10
    split_utime: uint32
    interval: uint32
= FutureSplitMerge;

fsm_merge$11
    merge_utime: uint32
    interval: uint32
= FutureSplitMerge;
*/
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum FutureSplitMerge {
    #[default]
    None,
    Split {
        split_utime: u32,
        interval: u32,
    },
    Merge {
        merge_utime: u32,
        interval: u32,
    }
}

impl Deserializable for FutureSplitMerge {
    fn read_from(&mut self, slice: &mut SliceData) -> Result<()> {
        if !slice.get_next_bit()? {
            *self = FutureSplitMerge::None;
        } else if !slice.get_next_bit()? {
            *self = FutureSplitMerge::Split {
                split_utime: slice.get_next_u32()?,
                interval: slice.get_next_u32()?,
            };
        } else {
            *self = FutureSplitMerge::Merge {
                merge_utime: slice.get_next_u32()?,
                interval: slice.get_next_u32()?,
            };
        }
        Ok(())
    }
}

impl Serializable for FutureSplitMerge {
    fn write_to(&self, cell: &mut BuilderData) -> Result<()> {
        match self {
            FutureSplitMerge::None => {
                cell.append_bit_zero()?;
            },
            FutureSplitMerge::Split { split_utime, interval } => {
                cell.append_bit_one()?;
                cell.append_bit_zero()?;
                split_utime.write_to(cell)?;
                interval.write_to(cell)?;
            },
            FutureSplitMerge::Merge { merge_utime, interval } => {
                cell.append_bit_one()?;
                cell.append_bit_one()?;
                merge_utime.write_to(cell)?;
                interval.write_to(cell)?;
            }
        }
        Ok(())
    }
}

// Current ser/de implementation for CollatorRange allows up to 9 validators in mempool 
// because all ranges are stored in one cell
pub const MEMPOOL_MAX_LEN: usize = 9;

#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct CollatorRange {
    pub collator: u16,
    pub mempool: smallvec::SmallVec<[u16; MEMPOOL_MAX_LEN]>,
    pub start: u32, // first block number which to collate
    pub finish: u32, // last block number which to callate
}

impl CollatorRange {
    pub fn range(&self) -> Range<u32> {
        self.start..self.finish
    }
}

impl fmt::Display for CollatorRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}..{})", self.collator, self.start, self.finish)
    }
}

impl Serializable for CollatorRange {
    fn write_with_opts(&self, builder: &mut BuilderData, opts: u8) -> Result<()> {
        if self.mempool.len() > MEMPOOL_MAX_LEN {
            fail!("Too many validators in mempool");
        }
        self.collator.write_to(builder)?;
        if opts & SERDE_OPTS_MEMPOOL_NODES != 0 {
            if self.mempool.len() > u8::MAX as usize {
                fail!("Too many validators in mempool");
            }
            builder.append_u8(self.mempool.len() as u8)?;
            for v in &self.mempool {
                v.write_to(builder)?;
            }
        }
        self.start.write_to(builder)?;
        self.finish.write_to(builder)?;
        Ok(())
    }

    fn write_to(&self, cell: &mut BuilderData) -> Result<()> {
        self.write_with_opts(cell, 0)
    }
}

impl Deserializable for CollatorRange {
    fn construct_from_with_opts(slice: &mut SliceData, opts: u8) -> Result<Self> {
        let collator = u16::construct_from(slice)?;
        let mempool = if opts & SERDE_OPTS_MEMPOOL_NODES != 0 {
            let len = slice.get_next_byte()? as usize;
            if len > MEMPOOL_MAX_LEN {
                fail!("Too many validators in mempool");
            }
            let mut vec = smallvec::SmallVec::<[u16; MEMPOOL_MAX_LEN]>::new();
            for _ in 0..len {
                vec.push(u16::construct_from(slice)?);
            }
            vec
        } else {
            smallvec::SmallVec::<[u16; MEMPOOL_MAX_LEN]>::new()
        };
        let start = slice.get_next_u32()?;
        let finish = slice.get_next_u32()?;
        Ok(Self {collator, mempool, start, finish})
    }

    fn construct_from(slice: &mut SliceData) -> Result<Self> {
        Self::construct_from_with_opts(slice, 0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct ShardCollators {
    pub prev: CollatorRange,
    pub prev2: Option<CollatorRange>,
    pub current: CollatorRange,
    pub next: CollatorRange,
    pub next2: Option<CollatorRange>,
    pub updated_at: u32,
    pub stat: ValidatorsStat
}

impl fmt::Display for ShardCollators {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "prev: {}", self.prev)?;
        if let Some(prev2) = &self.prev2 {
            writeln!(f, "prev2: {}", prev2)?;
        } else {
            writeln!(f, "prev2: none")?;
        }
        writeln!(f, "current: {}", self.current)?;
        writeln!(f, "next: {}", self.next)?;
        if let Some(next2) = &self.next2 {
            write!(f, "next2: {}", next2)?;
        } else {
            write!(f, "next2: none")?;
        }
        writeln!(f, "updated_at: {}", self.updated_at)?;
        Ok(())
    }
}

const SHARD_COLLATORS_TAG: u8 = 0x1; // 4 bits
const SHARD_COLLATORS_TAG_2: u8 = 0x2; // 4 bits

impl Serializable for ShardCollators {
    fn write_to(&self, builder: &mut BuilderData) -> Result<()> {
        let (tag, opts) = if self.stat.is_empty() {
            (SHARD_COLLATORS_TAG, SERDE_OPTS_EMPTY)
        } else {
            (SHARD_COLLATORS_TAG_2, SERDE_OPTS_MEMPOOL_NODES)
        };
        builder.append_bits(tag as usize, 4)?;
        self.prev.write_with_opts(builder, opts)?;
        self.prev2.write_with_opts(builder, opts)?;
        self.current.write_with_opts(builder, opts)?;
        self.next.write_with_opts(builder, opts)?;
        self.next2.write_with_opts(builder, opts)?;
        self.updated_at.write_with_opts(builder, opts)?;
        if !self.stat.is_empty() {
            self.stat.write_to_new_cell()?.into_cell()?.write_to(builder)?;
        }
        Ok(())
    }
}

impl Deserializable for ShardCollators {
    fn construct_from(slice: &mut SliceData) -> Result<Self> {
        let tag = slice.get_next_int(4)? as u8;
        if tag != SHARD_COLLATORS_TAG && tag != SHARD_COLLATORS_TAG_2 {
            fail!(
                BlockError::InvalidConstructorTag {
                    t: tag as u32,
                    s: std::any::type_name::<Self>().to_string()
                }
            )
        }
        let opts = if tag == SHARD_COLLATORS_TAG_2 {
            SERDE_OPTS_MEMPOOL_NODES
        } else {
            0
        };
        Ok(Self {
            prev: Deserializable::construct_from_with_opts(slice, opts)?,
            prev2: Deserializable::construct_from_with_opts(slice, opts)?,
            current: Deserializable::construct_from_with_opts(slice, opts)?,
            next: Deserializable::construct_from_with_opts(slice, opts)?,
            next2: Deserializable::construct_from_with_opts(slice, opts)?,
            updated_at: Deserializable::construct_from_with_opts(slice, opts)?,
            stat: if tag == SHARD_COLLATORS_TAG_2 {
                ValidatorsStat::construct_from(
                    &mut SliceData::load_cell(slice.checked_drain_reference()?)?)?
            } else {
                ValidatorsStat::default()
            }
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShardBlockRef {
    pub seq_no: u32,
    pub root_hash: UInt256,
    pub file_hash: UInt256,
    pub end_lt: u64,
}

impl Deserializable for ShardBlockRef {
    fn construct_from(slice: &mut SliceData) -> Result<Self> {
        Ok(Self {
            seq_no: slice.get_next_u32()?,
            root_hash: UInt256::construct_from(slice)?,
            file_hash: UInt256::construct_from(slice)?,
            end_lt: slice.get_next_u64()?,
        })
    }
}

impl Serializable for ShardBlockRef {
    fn write_to(&self, cell: &mut BuilderData) -> Result<()> {
        self.seq_no.write_to(cell)?;
        self.root_hash.write_to(cell)?;
        self.file_hash.write_to(cell)?;
        self.end_lt.write_to(cell)?;
        Ok(())
    }
}

impl ShardBlockRef {
    pub fn with_params(block_id: &BlockIdExt, end_lt: u64) -> Self {
        Self {
            seq_no: block_id.seq_no,
            root_hash: block_id.root_hash.clone(),
            file_hash: block_id.file_hash.clone(),
            end_lt,
        }
    }

    pub fn into_block_id(self, shard_id: ShardIdent) -> Result<BlockIdExt> {
        Ok(BlockIdExt {
            shard_id,
            seq_no: self.seq_no,
            root_hash: self.root_hash,
            file_hash: self.file_hash,
        })
    }
}

// workchain_id -> bintree_of_shards -> (seq_no, root_hash, file_hash)
define_HashmapE!{RefShardBlocks, 32, BinTree<ShardBlockRef>}

impl RefShardBlocks {
    pub fn with_ids<'a>(ids: impl IntoIterator<Item = &'a (BlockIdExt, u64)>) -> Result<Self> {
        // Naive implementation. 
        //TODO optimise me!

        let mut ref_shard_blocks = HashMap::new(); // wc -> shard -> id
        for (id, end_lt) in ids {
            let shards = loop {
                if let Some(wc) = ref_shard_blocks.get_mut(&id.shard().workchain_id()) {
                    break wc
                }
                ref_shard_blocks.insert(id.shard().workchain_id(), HashMap::new());
            };
            shards.insert(id.shard(), ShardBlockRef::with_params(id, *end_lt));
        }

        let mut result = Self::default();
        for (wc, mut shards) in ref_shard_blocks {
            let key = ShardIdent::full(wc);
            let mut bintree;
            if let Some(val) = shards.get(&key) {
                bintree = BinTree::with_item(val)?;
            } else {
                bintree = BinTree::with_item(&ShardBlockRef::default())?;
                let mut unfinished_keys = vec!(key);
                while let Some(key) = unfinished_keys.pop() {
                    bintree.split(key.shard_key(false), |_| {
                        let (left, right) = key.split()?;
                        let left_val = if let Some(val) = shards.remove(&left) {
                            val
                        } else {
                            unfinished_keys.push(left);
                            ShardBlockRef::default()
                        };
                        let right_val = if let Some(val) = shards.remove(&right) {
                            val
                        } else {
                            unfinished_keys.push(right);
                            ShardBlockRef::default()
                        };
                        Ok((left_val, right_val))
                    })?;
                }
                if !shards.is_empty() {
                    fail!("wrong ids (shards is not empty after bintree filling)")
                }
            }
            result.set(&wc, &bintree)?;
        }

        Ok(result)
    }

    pub fn iterate_shard_block_refs<F>(&self, mut func: F) -> Result<bool>
        where F: FnMut(BlockIdExt, u64) -> Result<bool> 
    {
        self.iterate_with_keys(|wc_id: i32, shards| {
            shards.iterate(|prefix, info| {
                let shard_ident = ShardIdent::with_prefix_slice(wc_id, prefix)?;
                let end_lt = info.end_lt;
                let block_id = info.into_block_id(shard_ident)?;
                func(block_id, end_lt)
            })
        })
    }

    pub fn ref_shard_block(&self, shard_ident: &ShardIdent) -> Result<Option<ShardBlockRef>> {
        if let Some(shards) = self.get(&shard_ident.workchain_id())? {
            if let Some(sbr) = shards.get(shard_ident.shard_key(false))? {
                return Ok(Some(sbr))
            }
        }
        Ok(None)
    }

}

define_HashmapE!(MeshHashesExt, 32, ConnectedNwDescrExt);

const CONNECTED_NW_DESCR_EXT_TAG: u8 = 1; // 4 bits

#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct ConnectedNwDescrExt {
    // Info about out queue from masterchain to connected network
    pub queue_descr: ConnectedNwOutDescr,
    pub descr: Option<ConnectedNwDescr>
}

impl Deserializable for ConnectedNwDescrExt {
    fn read_from(&mut self, slice: &mut SliceData) -> Result<()> {
        let tag = slice.get_next_int(4)? as u8;
        if tag != CONNECTED_NW_DESCR_EXT_TAG {
            fail!(
                BlockError::InvalidConstructorTag {
                    t: tag as u32,
                    s: std::any::type_name::<Self>().to_string()
                }
            )
        }
        self.queue_descr.read_from(slice)?;
        self.descr.read_from(slice)?;
        Ok(())
    }
}

impl Serializable for ConnectedNwDescrExt {
    fn write_to(&self, builder: &mut BuilderData) -> Result<()> {
        builder.append_bits(CONNECTED_NW_DESCR_EXT_TAG as usize, 4)?;
        self.queue_descr.write_to(builder)?;
        self.descr.write_to(builder)?;
        Ok(())
    }
}

define_HashmapE!(MeshOutDescr, 32, ConnectedNwOutDescr);

const CONNECTED_NW_QUEUE_DESCR_TAG: u8 = 1; // 4 bits

#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct ConnectedNwOutDescr {
    pub out_queue_update: HashUpdate,
    pub exported: VarUInteger32,
}

impl Deserializable for ConnectedNwOutDescr {
    fn read_from(&mut self, slice: &mut SliceData) -> Result<()> {
        let tag = slice.get_next_int(4)? as u8;
        if tag != CONNECTED_NW_QUEUE_DESCR_TAG {
            fail!(
                BlockError::InvalidConstructorTag {
                    t: tag as u32,
                    s: std::any::type_name::<Self>().to_string()
                }
            )
        }
        self.exported.read_from(slice)?;
        self.out_queue_update.read_from_cell(slice.checked_drain_reference()?)?;
        Ok(())
    }
}

impl Serializable for ConnectedNwOutDescr {
    fn write_to(&self, builder: &mut BuilderData) -> Result<()> {
        builder.append_bits(CONNECTED_NW_QUEUE_DESCR_TAG as usize, 4)?;
        self.exported.write_to(builder)?;
        builder.checked_append_reference(self.out_queue_update.serialize()?)?;
        Ok(())
    }
}

const PACK_INFO_TAG: u8 = 1; // 4 bits

#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct MsgPackProcessingInfo {
    pub round: u64,
    pub last_id: MsgPackId,
    pub last_partially_included: Option<UInt256>, // last included message hash, None if all messages were included
}

impl Serializable for MsgPackProcessingInfo {
    fn write_to(&self, builder: &mut BuilderData) -> Result<()> {
        builder.append_bits(PACK_INFO_TAG as usize, 4)?;
        self.round.write_to(builder)?;
        self.last_id.write_to(builder)?;
        self.last_partially_included.write_to(builder)?;
        Ok(())
    }
}

impl Deserializable for MsgPackProcessingInfo {
    fn read_from(&mut self, slice: &mut SliceData) -> Result<()> {
        let tag = slice.get_next_int(4)? as u8;
        if tag != PACK_INFO_TAG {
            fail!(
                BlockError::InvalidConstructorTag {
                    t: tag as u32,
                    s: std::any::type_name::<Self>().to_string()
                }
            )
        }
        self.round.read_from(slice)?;
        self.last_id.read_from(slice)?;
        self.last_partially_included.read_from(slice)?;
        Ok(())
    }
}

// Shard description (header)
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct ShardDescr {
    pub seq_no: u32,
    pub reg_mc_seqno: u32,
    pub start_lt: u64,
    pub end_lt: u64,
    pub root_hash: UInt256,
    pub file_hash: UInt256,
    pub before_split: bool,
    pub before_merge: bool,
    pub want_split: bool,
    pub want_merge: bool,
    pub nx_cc_updated: bool,
    pub flags: u8,
    pub next_catchain_seqno: u32,
    pub next_validator_shard: u64,
    pub min_ref_mc_seqno: u32,
    pub gen_utime: u32,
    pub split_merge_at: FutureSplitMerge,
    pub fees_collected: CurrencyCollection,
    pub funds_created: CurrencyCollection,
    pub copyleft_rewards: CopyleftRewards,
    pub proof_chain: Option<ProofChain>, // Some when CapWc2WcQueueUpdates is set
    pub collators: Option<ShardCollators>,
    pub mesh_msg_queues: MeshOutDescr,
    pub pack_info: Option<MsgPackProcessingInfo>,
}

impl ShardDescr {

    /// Constructs ShardDescr as slice with its params
    pub fn with_params(seq_no: u32, start_lt: u64, end_lt: u64, root_hash: UInt256, split_merge_at: FutureSplitMerge) -> Self {

        ShardDescr {
            seq_no,
            reg_mc_seqno: 0,
            start_lt,
            end_lt,
            root_hash,
            file_hash: UInt256::ZERO,
            before_split: false,
            before_merge: false,
            want_split: false,
            want_merge: false,
            nx_cc_updated: false,
            flags: 0,
            next_catchain_seqno: 0,
            next_validator_shard: 0,
            min_ref_mc_seqno: 0,
            gen_utime: 0,
            split_merge_at,
            fees_collected: CurrencyCollection::default(),
            funds_created: CurrencyCollection::default(),
            copyleft_rewards: CopyleftRewards::default(),
            proof_chain: None,
            collators: None,
            mesh_msg_queues: MeshOutDescr::default(),
            pack_info: None,
        }
    }
    pub fn fsm_equal(&self, other: &Self) -> bool {
        self.split_merge_at == other.split_merge_at
    }
    pub fn is_fsm_merge(&self) -> bool {
        matches!(self.split_merge_at, FutureSplitMerge::Merge{merge_utime: _, interval: _})
    }
    pub fn is_fsm_split(&self) -> bool {
        matches!(self.split_merge_at, FutureSplitMerge::Split{split_utime: _, interval: _})
    }
    pub fn is_fsm_none(&self) -> bool {
        matches!(self.split_merge_at, FutureSplitMerge::None)
    }
    pub fn fsm_utime(&self) -> u32 {
        match self.split_merge_at {
            FutureSplitMerge::Split{split_utime, interval: _} => split_utime,
            FutureSplitMerge::Merge{merge_utime, interval: _} => merge_utime,
            _ => 0
        }
    }
    pub fn fsm_utime_end(&self) -> u32 {
        match self.split_merge_at {
            FutureSplitMerge::Split{split_utime, interval} => split_utime + interval,
            FutureSplitMerge::Merge{merge_utime, interval} => merge_utime + interval,
            _ => 0
        }
    }
    pub fn fsm_interval(&self) -> u32 {
        match self.split_merge_at {
            FutureSplitMerge::Split{split_utime: _, interval} => interval,
            FutureSplitMerge::Merge{merge_utime: _, interval} => interval,
            _ => 0
        }
    }
    pub fn collators(&self) -> Result<&ShardCollators> {
        self.collators.as_ref().ok_or_else(|| error!("collators field is None"))
    }
    pub fn collators_mut(&mut self) -> Result<&mut ShardCollators> {
        self.collators.as_mut().ok_or_else(|| error!("collators field is None"))
    }
}

const SHARD_IDENT_TAG_A: u8 = 0xa; // 4 bit
const SHARD_IDENT_TAG_B: u8 = 0xb; // 4 bit
const SHARD_IDENT_TAG_C: u8 = 0xc; // 4 bit
const SHARD_IDENT_TAG_D: u8 = 0xd; // 4 bit // with all previous and proof chain
const SHARD_IDENT_TAG_E: u8 = 0xe; // 4 bit // with proof chain & collators & base shard blocks, without copyleft
const SHARD_IDENT_TAG_F: u8 = 0xf; // 4 bit // TAG_E + mesh_msg_queues
const SHARD_IDENT_TAG_G: u8 = 0x9; // 4 bit // TAG_F + pack_info
const SHARD_IDENT_TAG_LEN: usize = 4;

const SHARD_IDENT_TAGS: [u8; 7] = [
    SHARD_IDENT_TAG_A,
    SHARD_IDENT_TAG_B,
    SHARD_IDENT_TAG_C,
    SHARD_IDENT_TAG_D,
    SHARD_IDENT_TAG_E,
    SHARD_IDENT_TAG_F,
    SHARD_IDENT_TAG_G,
];

impl Deserializable for ShardDescr {
    fn read_from(&mut self, slice: &mut SliceData) -> Result<()> {
        let tag = slice.get_next_int(SHARD_IDENT_TAG_LEN)? as u8;
        if !SHARD_IDENT_TAGS.contains(&tag) {
            fail!(
                BlockError::InvalidConstructorTag {
                    t: tag as u32,
                    s: std::any::type_name::<Self>().to_string()
                } 
            )
        }

        self.seq_no.read_from(slice)?;
        self.reg_mc_seqno.read_from(slice)?;
        self.start_lt.read_from(slice)?;
        self.end_lt.read_from(slice)?;
        self.root_hash.read_from(slice)?;
        self.file_hash.read_from(slice)?;
        let mut flags: u8 = 0;
        flags.read_from(slice)?;
        self.before_split = (flags >> 7) & 1 == 1;
        self.before_merge = (flags >> 6) & 1 == 1;
        self.want_split = (flags >> 5) & 1 == 1;
        self.want_merge = (flags >> 4) & 1 == 1;
        self.nx_cc_updated = (flags >> 3) & 1 == 1;

        if (flags & 7) != 0 {
            fail!("flags & 7 in ShardDescr must be zero, but {}", flags)
        }

        self.next_catchain_seqno.read_from(slice)?;
        self.next_validator_shard.read_from(slice)?;
        self.min_ref_mc_seqno.read_from(slice)?;
        self.gen_utime.read_from(slice)?;
        self.split_merge_at.read_from(slice)?;
        match tag {
            SHARD_IDENT_TAG_B => {
                self.fees_collected.read_from(slice)?;
                self.funds_created.read_from(slice)?;
            }
            SHARD_IDENT_TAG_A => {
                let mut slice1 = SliceData::load_cell(slice.checked_drain_reference()?)?;
                self.fees_collected.read_from(&mut slice1)?;
                self.funds_created.read_from(&mut slice1)?;
            }
            SHARD_IDENT_TAG_C => {
                let mut slice1 = SliceData::load_cell(slice.checked_drain_reference()?)?;
                self.fees_collected.read_from(&mut slice1)?;
                self.funds_created.read_from(&mut slice1)?;
                self.copyleft_rewards.read_from(&mut slice1)?;
            }
            SHARD_IDENT_TAG_D => {
                let mut slice1 = SliceData::load_cell(slice.checked_drain_reference()?)?;
                self.fees_collected.read_from(&mut slice1)?;
                self.funds_created.read_from(&mut slice1)?;
                if slice1.get_next_bit()? {
                    self.copyleft_rewards.read_from(&mut slice1)?;
                }
                let proof_chain = ProofChain::construct_from(&mut slice1)?;
                self.proof_chain = Some(proof_chain);
            }
            SHARD_IDENT_TAG_E | SHARD_IDENT_TAG_F | SHARD_IDENT_TAG_G => {
                let mut slice1 = SliceData::load_cell(slice.checked_drain_reference()?)?;
                self.fees_collected.read_from(&mut slice1)?;
                self.funds_created.read_from(&mut slice1)?;
                self.proof_chain.read_from(&mut slice1)?;
                self.collators.read_from(&mut slice1)?;
                if tag == SHARD_IDENT_TAG_G {
                    let mut slice2 = SliceData::load_cell(slice1.checked_drain_reference()?)?;
                    self.pack_info.read_from(&mut slice2)?;
                }
            }
            _ => ()
        }
        if tag == SHARD_IDENT_TAG_F {
            self.mesh_msg_queues.read_from(slice)?;
        }

        Ok(())
    }
}

impl Serializable for ShardDescr {
    fn write_to(&self, cell: &mut BuilderData) -> Result<()> {
        let mut tag = SHARD_IDENT_TAG_A; // TAG_B is not used at all.
        
        if self.pack_info.is_some() {
            tag = SHARD_IDENT_TAG_G;
        } else if !self.mesh_msg_queues.is_empty() {
            tag = SHARD_IDENT_TAG_F;
        } else if self.collators.is_some() {
            tag = SHARD_IDENT_TAG_E;
        } else if self.proof_chain.is_some() {
            tag = SHARD_IDENT_TAG_D;
        } else if !self.copyleft_rewards.is_empty() {
            tag = SHARD_IDENT_TAG_C
        }

        cell.append_bits(tag as usize, SHARD_IDENT_TAG_LEN)?;

        self.seq_no.write_to(cell)?;
        self.reg_mc_seqno.write_to(cell)?;
        self.start_lt.write_to(cell)?;
        self.end_lt.write_to(cell)?;
        self.root_hash.write_to(cell)?;
        self.file_hash.write_to(cell)?;

        let mut flags: u8 = 0;
        if self.before_split {
            flags |= 1 << 7
        }
        if self.before_merge {
            flags |= 1 << 6;
        }
        if self.want_split {
            flags |= 1 << 5;
        }
        if self.want_merge {
            flags |= 1 << 4;
        }
        if self.nx_cc_updated {
            flags |= 1 << 3;
        }
        if (self.flags & 7) != 0 {
            fail!("flags & 7 must be zero, but it {}", self.flags)
        }

        flags.write_to(cell)?;

        self.next_catchain_seqno.write_to(cell)?;
        self.next_validator_shard.write_to(cell)?;
        self.min_ref_mc_seqno.write_to(cell)?;
        self.gen_utime.write_to(cell)?;
        self.split_merge_at.write_to(cell)?;

        let mut child = BuilderData::new();
        self.fees_collected.write_to(&mut child)?;
        self.funds_created.write_to(&mut child)?;
        match tag {
            SHARD_IDENT_TAG_E | SHARD_IDENT_TAG_F | SHARD_IDENT_TAG_G => {
                if !self.copyleft_rewards.is_empty() {
                    fail!("copyleft_rewards is not supported with 'collators' or 'mesh_msg_queues'")
                }
                self.proof_chain.write_to(&mut child)?;
                self.collators.write_to(&mut child)?;
                if tag == SHARD_IDENT_TAG_G {
                    let mut child2 = BuilderData::new();
                    self.pack_info.write_to(&mut child2)?;
                    child.checked_append_reference(child2.into_cell()?)?;
                }
            }
            SHARD_IDENT_TAG_D => {
                let proof_chain = self.proof_chain.as_ref()
                    .ok_or_else(|| error!("INTARNAL ERROR: proof_chain is None"))?;
                if !self.copyleft_rewards.is_empty() {
                    child.append_bit_one()?;
                    self.copyleft_rewards.write_to(&mut child)?;
                } else {
                    child.append_bit_zero()?;
                }
                proof_chain.write_to(&mut child)?;
            }
            SHARD_IDENT_TAG_C => {
                self.copyleft_rewards.write_to(&mut child)?;
            }
            _ => ()
        }
        cell.checked_append_reference(child.into_cell()?)?;
        if !self.mesh_msg_queues.is_empty() {
            self.mesh_msg_queues.write_to(cell)?;
        }

        Ok(())
    }
}

/*
master_info$_ master:ExtBlkRef = BlkMasterInfo;
*/
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct BlkMasterInfo {
    pub master: ExtBlkRef
}

impl Deserializable for BlkMasterInfo {
     fn read_from(&mut self, cell: &mut SliceData) -> Result<()> {
        self.master.read_from(cell)
    }
}

impl Serializable for BlkMasterInfo {
    fn write_to(&self, cell: &mut BuilderData) -> Result<()> {
        self.master.write_to(cell)
    }
}


define_HashmapE!(Publishers, 256, ());
/*
shared_lib_descr$00 lib:^Cell publishers:(Hashmap 256 True) = LibDescr;
*/
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LibDescr {
    lib: Cell,
    publishers: Publishers
}

impl LibDescr {
    pub fn new(lib: Cell) -> Self {
        Self {
            lib,
            publishers: Publishers::default()
        }
    }
    pub fn from_lib_data_by_publisher(lib: Cell, publisher: AccountId) -> Self {
        let mut publishers = Publishers::default();
        publishers.set(&publisher, &()).unwrap();
        Self {
            lib,
            publishers
        }
    }
    pub fn publishers(&self) -> &Publishers {
        &self.publishers
    }
    pub fn publishers_mut(&mut self) -> &mut Publishers {
        &mut self.publishers
    }
    pub fn lib(&self) -> &Cell {
        &self.lib
    }
}

impl Deserializable for LibDescr {
    fn read_from(&mut self, slice: &mut SliceData) -> Result<()> {
        let tag = slice.get_next_int(2)?;
        if tag != 0 {
            fail!(
                BlockError::InvalidConstructorTag {
                    t: tag as u32,
                    s: std::any::type_name::<Self>().to_string()
                }
            )
        }
        self.lib.read_from(slice)?;
        self.publishers.read_hashmap_root(slice)?;
        Ok(())
    }
}

impl Serializable for LibDescr {
    fn write_to(&self, cell: &mut BuilderData) -> Result<()> {
        if self.publishers.is_empty() {
            fail!(BlockError::InvalidData("self.publishers is empty".to_string()))
        }
        cell.append_bits(0, 2)?;
        self.lib.write_to(cell)?;
        self.publishers.write_hashmap_root(cell)?;
        Ok(())
    }
}
