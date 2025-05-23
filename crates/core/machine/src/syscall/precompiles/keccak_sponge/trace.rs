use crate::syscall::precompiles::keccak_sponge::columns::{
    KeccakSpongeCols, NUM_KECCAK_SPONGE_COLS,
};
use crate::syscall::precompiles::keccak_sponge::utils::keccakf_u32s;
use crate::syscall::precompiles::keccak_sponge::{
    KeccakSpongeChip, KECCAK_GENERAL_OUTPUT_U32S, KECCAK_GENERAL_RATE_U32S, KECCAK_STATE_U32S,
};

use hashbrown::HashMap;
use itertools::Itertools;
use p3_field::PrimeField32;
use p3_keccak_air::{generate_trace_rows, NUM_KECCAK_COLS, NUM_ROUNDS};
use p3_matrix::dense::RowMajorMatrix;
use p3_matrix::Matrix;
use p3_maybe_rayon::prelude::{ParallelIterator, ParallelSlice};
use std::borrow::BorrowMut;
use zkm_core_executor::events::{ByteLookupEvent, ByteRecord, KeccakSpongeEvent, PrecompileEvent};
use zkm_core_executor::syscalls::SyscallCode;
use zkm_core_executor::{ExecutionRecord, Program};
use zkm_stark::{MachineAir, Word};

impl<F: PrimeField32> MachineAir<F> for KeccakSpongeChip {
    type Record = ExecutionRecord;
    type Program = Program;

    fn name(&self) -> String {
        "KeccakSponge".to_string()
    }

    fn generate_dependencies(&self, input: &Self::Record, output: &mut Self::Record) {
        let events = input.get_precompile_events(SyscallCode::KECCAK_SPONGE);
        let chunk_size = std::cmp::max(events.len() / num_cpus::get(), 1);

        let blu_batches = events
            .par_chunks(chunk_size)
            .map(|events| {
                let mut blu: HashMap<ByteLookupEvent, usize> = HashMap::new();
                events.iter().for_each(|(_, event)| {
                    let event = if let PrecompileEvent::KeccakSponge(event) = event {
                        event
                    } else {
                        unreachable!()
                    };
                    self.event_to_rows::<F>(event, &mut None, &mut blu);
                });
                blu
            })
            .collect::<Vec<_>>();

        output.add_byte_lookup_events_from_maps(blu_batches.iter().collect_vec());
    }

    fn generate_trace(&self, input: &Self::Record, _: &mut Self::Record) -> RowMajorMatrix<F> {
        let rows = Vec::new();

        let mut wrapped_rows = Some(rows);
        for (_, event) in input.get_precompile_events(SyscallCode::KECCAK_SPONGE) {
            let event = if let PrecompileEvent::KeccakSponge(event) = event {
                event
            } else {
                unreachable!()
            };
            self.event_to_rows(event, &mut wrapped_rows, &mut Vec::new());
        }
        let mut rows = wrapped_rows.unwrap();
        let num_real_rows = rows.len();

        let dummy_keccak_rows = generate_trace_rows::<F>(vec![[0; KECCAK_STATE_U32S / 2]]);
        let mut dummy_chunk = Vec::new();
        for i in 0..NUM_ROUNDS {
            let dummy_row = dummy_keccak_rows.row(i);
            let mut row = [F::ZERO; NUM_KECCAK_SPONGE_COLS];
            row[..NUM_KECCAK_COLS].copy_from_slice(dummy_row.collect::<Vec<_>>().as_slice());
            dummy_chunk.push(row);
        }

        let num_padded_rows = num_real_rows.next_power_of_two();
        for i in num_real_rows..num_padded_rows {
            let dummy_row = dummy_chunk[i % NUM_ROUNDS];
            rows.push(dummy_row);
        }

        RowMajorMatrix::new(rows.into_iter().flatten().collect::<Vec<_>>(), NUM_KECCAK_SPONGE_COLS)
    }

    fn included(&self, shard: &Self::Record) -> bool {
        if let Some(shape) = shard.shape.as_ref() {
            shape.included::<F, _>(self)
        } else {
            !shard.get_precompile_events(SyscallCode::KECCAK_SPONGE).is_empty()
        }
    }
}

impl KeccakSpongeChip {
    pub fn event_to_rows<F: PrimeField32>(
        &self,
        event: &KeccakSpongeEvent,
        rows: &mut Option<Vec<[F; NUM_KECCAK_SPONGE_COLS]>>,
        blu: &mut impl ByteRecord,
    ) {
        let mut state_u32s = [0_u32; KECCAK_STATE_U32S];
        let mut xored_rate_u32s = [0_u32; KECCAK_GENERAL_RATE_U32S];
        let block_num = event.num_blocks();
        let mut already_absorbed_u32s = 0_u32;

        for i in 0..block_num {
            let p3_keccak_trace = generate_trace_rows::<F>(vec![event.xored_state_list[i]]);
            for round in 0..NUM_ROUNDS {
                let mut row = [F::ZERO; NUM_KECCAK_SPONGE_COLS];
                let p3_keccak_row = p3_keccak_trace.row(round);
                row[..NUM_KECCAK_COLS]
                    .copy_from_slice(p3_keccak_row.collect::<Vec<_>>().as_slice());

                let cols: &mut KeccakSpongeCols<F> = row.as_mut_slice().borrow_mut();

                cols.shard = F::from_canonical_u32(event.shard);
                cols.clk = F::from_canonical_u32(event.clk);
                cols.is_real = F::ONE;
                cols.input_len = F::from_canonical_u32(event.input.len() as u32);
                cols.already_absorbed_u32s = F::from_canonical_u32(already_absorbed_u32s);
                cols.is_absorbed =
                    F::from_bool((round == (NUM_ROUNDS - 1)) && (i != (block_num - 1)));
                cols.is_first_input_block = F::from_bool(i == 0);
                cols.is_final_input_block = F::from_bool(i == (block_num - 1));
                cols.read_block = F::from_bool(round == 0);
                cols.receive_syscall = F::from_bool(i == 0 && round == 0);
                cols.write_output = F::from_bool(i == (block_num - 1) && round == (NUM_ROUNDS - 1));
                cols.output_address = F::from_canonical_u32(event.output_addr);
                // 4 bytes per u32
                cols.input_address = F::from_canonical_u32(
                    event.input_addr + i as u32 * KECCAK_GENERAL_RATE_U32S as u32 * 4,
                );

                // read the input
                if round == 0 {
                    for j in 0..KECCAK_GENERAL_RATE_U32S {
                        cols.block_mem[j].populate(
                            event.input_read_records[i * KECCAK_GENERAL_RATE_U32S + j],
                            blu,
                        );
                    }
                }

                // original state
                for j in 0..KECCAK_STATE_U32S {
                    cols.original_state[j] = Word::from(state_u32s[j]);
                }

                // xor
                if round == 0 {
                    for j in 0..KECCAK_GENERAL_RATE_U32S {
                        xored_rate_u32s[j] = cols.xored_general_rate[j].populate(
                            blu,
                            state_u32s[j],
                            event.input[i * KECCAK_GENERAL_RATE_U32S + j],
                        );
                    }
                }

                // if this is the first round of the first block, populate reading input length
                if i == 0 && round == 0 {
                    cols.input_length_mem.populate(event.input_length_record, blu);
                }

                // if this is the last row of the last block, populate writing output
                if i == (block_num - 1) && round == (NUM_ROUNDS - 1) {
                    for j in 0..KECCAK_GENERAL_OUTPUT_U32S {
                        cols.output_mem[j].populate(event.output_write_records[j], blu);
                    }
                }

                if rows.as_ref().is_some() {
                    rows.as_mut().unwrap().push(row);
                }
            }
            state_u32s[..KECCAK_GENERAL_RATE_U32S].copy_from_slice(&xored_rate_u32s[..]);
            keccakf_u32s(&mut state_u32s);
            already_absorbed_u32s += KECCAK_GENERAL_RATE_U32S as u32;
        }
    }
}
