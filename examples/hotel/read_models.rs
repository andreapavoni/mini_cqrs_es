use mini_cqrs_es::toasty;

#[derive(Debug, toasty::Model)]
#[table = "hotel_read_models"]
pub struct HotelReadModel {
    #[key]
    pub id: String,
    pub room_count: u32,
}

#[derive(Debug, toasty::Model)]
#[table = "room_read_models"]
#[key(hotel_id, room_number)]
pub struct RoomReadModel {
    pub hotel_id: String,
    pub room_number: u32,
    pub guest_name: Option<String>,
}
