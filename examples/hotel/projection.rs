use super::{
    domain::{HotelAggregate, HotelEvent},
    read_models::{HotelReadModel, RoomReadModel},
};
use mini_cqrs_es::{CqrsError, Projection, StoredEvent, toasty};

#[derive(Clone)]
pub struct HotelProjection;

impl Projection for HotelProjection {
    type Aggregate = HotelAggregate;

    fn name() -> &'static str {
        "hotel"
    }

    async fn apply(
        &self,
        event: &StoredEvent,
        payload: HotelEvent,
        executor: &mut dyn toasty::Executor,
    ) -> Result<(), CqrsError> {
        match payload {
            HotelEvent::HotelInitialized { room_count } => {
                HotelReadModel::upsert_by_id(&event.aggregate_id)
                    .room_count(room_count)
                    .exec(&mut *executor)
                    .await
                    .map_err(|error| CqrsError::storage("hotel projection", error))?;
                RoomReadModel::filter_by_hotel_id(&event.aggregate_id)
                    .delete()
                    .exec(&mut *executor)
                    .await
                    .map_err(|error| CqrsError::storage("hotel projection", error))?;
                for room_number in 1..=room_count {
                    toasty::create!(RoomReadModel {
                        hotel_id: event.aggregate_id.clone(),
                        room_number,
                        guest_name: None::<String>,
                    })
                    .exec(&mut *executor)
                    .await
                    .map_err(|error| CqrsError::storage("hotel projection", error))?;
                }
            }
            HotelEvent::GuestCheckedIn {
                room_number,
                guest_name,
            } => {
                let mut room = RoomReadModel::get_by_hotel_id_and_room_number(
                    &mut *executor,
                    &event.aggregate_id,
                    room_number,
                )
                .await
                .map_err(|error| CqrsError::storage("hotel projection", error))?;
                room.update()
                    .guest_name(Some(guest_name))
                    .exec(&mut *executor)
                    .await
                    .map_err(|error| CqrsError::storage("hotel projection", error))?;
            }
            HotelEvent::GuestCheckedOut { room_number } => {
                let mut room = RoomReadModel::get_by_hotel_id_and_room_number(
                    &mut *executor,
                    &event.aggregate_id,
                    room_number,
                )
                .await
                .map_err(|error| CqrsError::storage("hotel projection", error))?;
                room.update()
                    .guest_name(None::<String>)
                    .exec(&mut *executor)
                    .await
                    .map_err(|error| CqrsError::storage("hotel projection", error))?;
            }
        }
        Ok(())
    }
}
