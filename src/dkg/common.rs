use crate::*;

pub fn partition_domain<A>(
    params: &Params,
    announce_messages: &mut Vec<A>,
) -> Result<Vec<A::Participant>>
where
    A: Announcement,
{
    print_time!("Partition domain");
    // Sort participants from greatest to least stake
    announce_messages.sort_by(|a, b| b.stake().cmp(&a.stake()));
    // Compute the total amount staked
    let total_stake: f64 = announce_messages
        .iter()
        .map(|p| p.stake() as f64)
        .sum::<f64>()
        .into();

    // Compute the weight of each participant rounded down
    let mut weights = announce_messages
        .iter()
        .map(|p| {
            ((params.total_weight as f64) * p.stake() as f64 / total_stake)
                .floor() as u32
        })
        .collect::<Vec<u32>>();
    //dbg!(&weights);
    // Add any excess weight to the largest weight participants
    let adjust_weight = params
        .total_weight
        .checked_sub(weights.iter().sum())
        .ok_or_else(|| anyhow!("adjusted weight negative"))?
        as usize;
    for i in &mut weights[0..adjust_weight] {
        *i += 1;
    }

    // total_weight is allocated among all participants
    assert_eq!(weights.iter().sum::<u32>(), params.total_weight);

    let mut allocated_weight = 0usize;
    let mut participants = vec![];
    for (announcement, weight) in announce_messages.iter().zip(weights) {
        let share_range = allocated_weight..allocated_weight + weight as usize;

        participants.push(announcement.participant(weight, share_range));
        allocated_weight = allocated_weight
            .checked_add(weight as usize)
            .ok_or_else(|| anyhow!("allocated weight overflow"))?;
    }
    Ok(participants)
}