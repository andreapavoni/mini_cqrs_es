use super::{
    domain::{HotelId, RoomState},
    read_models::RoomReadModel,
};
use mini_cqrs_es::{CqrsError, Query, toasty};
use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HotelState {
    pub rooms: HashMap<u32, RoomState>,
}

#[derive(Clone)]
pub struct GetHotelStateQuery {
    id: HotelId,
    database: toasty::Db,
}

impl GetHotelStateQuery {
    pub fn new(id: HotelId, database: toasty::Db) -> Self {
        Self { id, database }
    }
}

impl Query for GetHotelStateQuery {
    type Output = Result<HotelState, CqrsError>;
    async fn apply(&self) -> Self::Output {
        let mut database = self.database.clone();
        let rows = RoomReadModel::filter_by_hotel_id(self.id.to_string())
            .exec(&mut database)
            .await
            .map_err(|error| CqrsError::storage("hotel query", error))?;
        Ok(HotelState {
            rooms: rows
                .into_iter()
                .map(|row| {
                    (
                        row.room_number,
                        row.guest_name
                            .map_or(RoomState::Free, |guest_name| RoomState::Occupied {
                                guest_name,
                            }),
                    )
                })
                .collect(),
        })
    }
}
