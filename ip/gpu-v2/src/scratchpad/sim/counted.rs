//! Four typed bank stores retain all DMA/core payload and address provenance.
use audited::{Fixed, FrameReport, Model};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transaction {
    DmaWrite { address: usize, data: u64 },
    CoreWrite { address: usize, data: u64 },
    CoreRead { address: usize },
}
impl Transaction {
    pub fn address(self) -> usize {
        match self {
            Self::DmaWrite { address, .. }
            | Self::CoreWrite { address, .. }
            | Self::CoreRead { address } => address,
        }
    }
}
pub struct Report {
    pub frame: FrameReport,
    pub reads: Vec<u64>,
    pub transactions: Vec<Transaction>,
    pub vertex_reads: Vec<VertexRead>,
    pub vertices: Vec<u128>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VertexRead {
    pub first: usize,
    pub second: usize,
    pub high_half: bool,
}
pub fn run(transactions: &[Transaction]) -> Result<Report, String> {
    run_with_vertices(transactions, &[])
}
pub fn run_with_vertices(
    transactions: &[Transaction],
    vertex_reads: &[VertexRead],
) -> Result<Report, String> {
    if transactions.is_empty()
        || transactions.len() > 4096
        || vertex_reads.len() > 4096
        || transactions
            .iter()
            .any(|t| t.address() % 8 != 0 || t.address() > 8184)
    {
        return Err("scratchpad transaction bounds".into());
    }
    let mut model = Model::numerical();
    let addresses = model
        .input::<14, 0, false>(
            "addresses",
            &transactions
                .iter()
                .map(|t| t.address() as i128)
                .collect::<Vec<_>>(),
        )
        .map_err(|e| format!("{e:?}"))?;
    let words = model
        .input::<64, 0, false>(
            "system_beats",
            &transactions
                .iter()
                .map(|t| match t {
                    Transaction::DmaWrite { data, .. } | Transaction::CoreWrite { data, .. } => {
                        i128::from(*data)
                    }
                    _ => 0,
                })
                .collect::<Vec<_>>(),
        )
        .map_err(|e| format!("{e:?}"))?;
    let mut banks = Vec::new();
    for b in 0..4 {
        banks.push(
            model
                .scratch::<16, 0, false>(&format!("SP{b}"), 1024)
                .map_err(|e| format!("{e:?}"))?,
        );
    }
    let f = model
        .compute(
            "scratchpad",
            transactions.len() * 64 + vertex_reads.len() * 16 + 64,
        )
        .map_err(|e| format!("{e:?}"))?;
    let mut index = Fixed::<13, 0, false>::constant::<0>();
    let mut read_results = Vec::new();
    let mut execute = || -> Result<(), audited::Fault> {
        for (i, transaction) in transactions.iter().enumerate() {
            let address = f.read(addresses.indexed(index))?;
            let row = f.slice::<10, 0, false, 3>(address)?;
            match transaction {
                Transaction::DmaWrite { .. } | Transaction::CoreWrite { .. } => {
                    let word = f.read(words.indexed(index))?;
                    let parts = [
                        f.slice::<16, 0, false, 0>(word)?,
                        f.slice::<16, 0, false, 16>(word)?,
                        f.slice::<16, 0, false, 32>(word)?,
                        f.slice::<16, 0, false, 48>(word)?,
                    ];
                    for b in 0..4 {
                        f.write(banks[b].indexed(row), parts[b])?;
                    }
                }
                Transaction::CoreRead { .. } => {
                    let mut parts = [Fixed::<64, 0, false>::constant::<0>(); 4];
                    for b in 0..4 {
                        parts[b] = f.resize_exact(f.read(banks[b].indexed(row))?)?;
                    }
                    let word = f.add_same(
                        f.add_same(parts[0], f.shift_left_const::<16, 64, 0, false>(parts[1])?)?,
                        f.add_same(
                            f.shift_left_const::<32, 64, 0, false>(parts[2])?,
                            f.shift_left_const::<48, 64, 0, false>(parts[3])?,
                        )?,
                    )?;
                    f.publish(&format!("read.{i}"), word)?;
                    read_results.push(word);
                }
            }
            index = f.add_same(index, Fixed::<13, 0, false>::constant::<1>())?;
        }
        for (i, packet) in vertex_reads.iter().enumerate() {
            let first = *read_results
                .get(packet.first)
                .ok_or(audited::Fault::Range)?;
            let second = *read_results
                .get(packet.second)
                .ok_or(audited::Fault::Range)?;
            let packed = if packet.high_half {
                let low = f.resize_exact::<96, 0, false>(f.slice::<32, 0, false, 32>(first)?)?;
                let high = f.shift_left_const::<32, 96, 0, false>(f.resize_exact(second)?)?;
                f.add_same(low, high)?
            } else {
                let low = f.resize_exact::<96, 0, false>(first)?;
                let high = f.shift_left_const::<64, 96, 0, false>(
                    f.resize_exact(f.slice::<32, 0, false, 0>(second)?)?,
                )?;
                f.add_same(low, high)?
            };
            f.publish(&format!("vertex_packet.{i}"), packed)?;
        }
        Ok(())
    };
    execute().map_err(|e| format!("{e:?}"))?;
    let frame = f.finish();
    frame.audit().map_err(|e| format!("{e:?}"))?;
    let reads = frame
        .outputs
        .iter()
        .filter(|o| o.name.starts_with("read."))
        .map(|o| o.raw as u64)
        .collect();
    let vertices = frame
        .outputs
        .iter()
        .filter(|o| o.name.starts_with("vertex_packet."))
        .map(|o| o.raw as u128)
        .collect();
    Ok(Report {
        frame,
        reads,
        transactions: transactions.to_vec(),
        vertex_reads: vertex_reads.to_vec(),
        vertices,
    })
}
