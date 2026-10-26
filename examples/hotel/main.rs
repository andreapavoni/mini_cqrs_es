//! A complete CQRS application. See examples/hotel/README.md for commands/configuration.
mod app;
mod config;

use app::*;
use mini_cqrs_es::*;

fn id(value: &str) -> anyhow::Result<HotelId> {
    Ok(HotelId::new(value.parse()?))
}

async fn execute<C: Command<Aggregate = HotelAggregate>>(
    runner: &impl Cqrs<HotelAggregate>,
    hotel: &HotelId,
    command: &C,
) -> anyhow::Result<()> {
    match runner
        .execute(hotel, command, EventMetadata::default())
        .await
    {
        Ok(receipt) => println!(
            "Hotel {} committed version {} ({} events)",
            receipt.aggregate_id,
            receipt.version,
            receipt.event_ids.len()
        ),
        Err(error) => {
            if let ExecutionError::Committed { receipt, stage, .. } = &error {
                eprintln!(
                    "Events committed at version {}; {stage:?} failed. Fix the cause and use catch-up; do not repeat the command.",
                    receipt.version
                );
            }
            return Err(error.into());
        }
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("help");
    if command == "help" {
        println!(
            "hotel init | create ID ROOMS | check-in ID ROOM GUEST | check-out ID ROOM | show ID | catch-up ID | rebuild TARGET_URL"
        );
        return Ok(());
    }
    let config = config::Config::from_env()?;
    let events = EventLog::connect(&config.events_url).await?;
    let read_models = || {
        ReadModels::builder()
            .register::<HotelReadModel>()
            .register::<RoomReadModel>()
    };
    if command == "rebuild" {
        let target = args
            .get(1)
            .ok_or_else(|| anyhow::anyhow!("rebuild requires a fresh, provisioned TARGET_URL"))?;
        anyhow::ensure!(
            target != &config.events_url && target != &config.reads_url,
            "replacement must be separate from active databases"
        );
        // The operator pauses all writers/delivery across prepare, replay and cutover.
        let prepared = ReplayPlan::new()
            .with_aggregate::<HotelAggregate>()
            .prepare(&events)
            .await?;
        let replacement = read_models().connect(target).await?;
        replacement.create_schema().await?;
        let progress = prepared
            .run(&EventConsumers::new().with(replacement.projector(HotelProjection)))
            .await?;
        println!(
            "Rebuilt {} streams / {} events. Publish only after success; configure READ_MODELS_DATABASE_URL for cutover.",
            progress.streams, progress.events
        );
        return Ok(());
    }
    let reads = read_models().connect(&config.reads_url).await?;
    if command == "init" {
        // Explicit first-time setup of fresh/provisioned databases. Connection alone does no DDL.
        events.create_schema().await?;
        reads.create_schema().await?;
        println!("Created event/support and read-model schemas.");
        return Ok(());
    }
    let projection = reads.projector(HotelProjection);
    let runner = SimpleCqrs::new(events.clone())
        .with_snapshots(events.snapshot_cache())
        .with_consumer(projection.clone());
    match (command, &args[1..]) {
        ("create", [hotel, rooms]) => {
            execute(
                &runner,
                &id(hotel)?,
                &CmdInitializeHotel {
                    room_count: rooms.parse()?,
                },
            )
            .await?
        }
        ("check-in", [hotel, room, guest]) => {
            execute(
                &runner,
                &id(hotel)?,
                &CmdCheckIn {
                    room_number: room.parse()?,
                    guest_name: guest.clone(),
                },
            )
            .await?
        }
        ("check-out", [hotel, room]) => {
            execute(
                &runner,
                &id(hotel)?,
                &CmdCheckOut {
                    room_number: room.parse()?,
                },
            )
            .await?
        }
        ("show", [hotel]) => {
            let state = runner
                .query(&GetHotelStateQuery::new(id(hotel)?, reads.database()))
                .await?;
            let mut rooms: Vec<_> = state.rooms.into_iter().collect();
            rooms.sort_by_key(|(room, _)| *room);
            for (room, state) in rooms {
                println!("Room {room}: {state:?}");
            }
        }
        ("catch-up", [hotel]) => println!(
            "Projected through version {}",
            projection.catch_up(&events, &id(hotel)?).await?
        ),
        _ => anyhow::bail!("invalid command or arguments; run hotel help"),
    }
    Ok(())
}
