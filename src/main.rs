use std::{collections::HashSet, io::Cursor};

use cached::proc_macro::cached;
use chrono::{NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Europe::Berlin;
use ics::{
    escape_text,
    properties::{
        Categories, Description, DtEnd, DtStart, Duration, Location as IcsLocation, Organizer,
        Summary,
    },
    Event as IcsEvent, ICalendar,
};
use reqwest::{header::CONTENT_DISPOSITION, Client, Url};
use rocket::{
    http::{ContentType, Header},
    response::Responder,
    serde::json::Json,
    Response,
};
use serde::{Deserialize, Deserializer, Serialize};

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate rocket;

const DATE_FORMAT: &str = "%Y%m%dT%H%M%SZ";
const ACADEMIC_YEAR_NAMES: [&str; 4] = ["1. Lehrjahr", "2. Lehrjahr", "3. Lehrjahr", "Wahlpflicht"];

lazy_static! {
    static ref REQWEST_CLIENT: Client = Client::new();
    static ref MATSE_SCHEDULE_URL: Url =
        Url::parse("https://www.matse.itc.rwth-aachen.de/stundenplan/web/eventFeed/").unwrap();
}

#[derive(Hash, PartialEq, Eq, Clone, FromForm)]
struct Semester {
    year: i32,
    winter_semester: bool,
}

impl Semester {
    fn get_start_date(&self) -> Option<NaiveDate> {
        if self.winter_semester {
            NaiveDate::from_ymd_opt(self.year, 9, 1)
        } else {
            NaiveDate::from_ymd_opt(self.year, 3, 1)
        }
    }

    fn get_end_date(&self) -> Option<NaiveDate> {
        if self.winter_semester {
            NaiveDate::from_ymd_opt(self.year + 1, 3, 15)
        } else {
            NaiveDate::from_ymd_opt(self.year, 9, 15)
        }
    }
}

#[derive(Clone, Deserialize)]
struct Event {
    name: String,
    #[serde(deserialize_with = "naive_from_berlin")]
    start: NaiveDateTime,
    #[serde(deserialize_with = "naive_from_berlin")]
    end: NaiveDateTime,
    location: Location,
    lecturer: Lecturer,
    information: Option<String>,
    #[serde(deserialize_with = "bool_from_str_option", rename = "isHoliday")]
    is_holiday: bool,
    #[serde(deserialize_with = "bool_from_str_option", rename = "isExercise")]
    is_exercise: bool,
    #[serde(default, rename = "allDay")]
    is_all_day: bool,
    #[serde(deserialize_with = "bool_from_str_option", rename = "isLecture")]
    is_lecture: bool,
}

impl Event {
    fn get_start_date(&self) -> String {
        self.start.format(DATE_FORMAT).to_string()
    }

    fn get_end_date(&self) -> String {
        self.end.format(DATE_FORMAT).to_string()
    }
}

#[derive(Clone, Deserialize)]
struct Location {
    name: Option<String>,
    street: Option<String>,
    nr: Option<String>,
    desc: Option<String>,
}

impl ToString for Location {
    fn to_string(&self) -> String {
        let empty = String::from("");
        let name = self
            .name
            .as_ref()
            .map(|name| format!("{}\n", name))
            .unwrap_or_default();
        let address = self
            .street
            .as_ref()
            .map(|street| format!("{} {}\n", street, self.nr.as_ref().unwrap_or(&empty)))
            .unwrap_or_default();
        format!(
            "{}{}{}",
            name,
            address,
            self.desc.as_ref().unwrap_or(&empty)
        )
        .trim()
        .into()
    }
}

impl Location {
    fn contains_information(&self) -> bool {
        self.name.is_some() || self.street.is_some() || self.desc.is_some()
    }
}

#[derive(Clone, Deserialize)]
struct Lecturer {
    name: Option<String>,
    mail: Option<String>,
}

impl ToString for Lecturer {
    fn to_string(&self) -> String {
        match (self.name.as_ref(), self.mail.as_ref()) {
            (Some(name), Some(mail)) => format!("CN={}:MAILTO:{}", name, mail),
            (Some(name), None) => format!("CN={}", name),
            (None, Some(mail)) => format!(":MAILTO:{}", mail),
            _ => "".into(),
        }
    }
}

impl Lecturer {
    fn contains_information(&self) -> bool {
        self.name.is_some() || self.mail.is_some()
    }
}

fn bool_from_str_option<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::deserialize(deserializer)? {
        Some("0") | None => Ok(false),
        Some(_) => Ok(true),
    }
}

fn naive_from_berlin<'de, D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
where
    D: Deserializer<'de>,
{
    let date = NaiveDateTime::deserialize(deserializer)?;
    let date = Berlin.from_local_datetime(&date).unwrap();
    Ok(date.naive_utc())
}

impl<'a> From<Event> for IcsEvent<'a> {
    fn from(event: Event) -> Self {
        let mut ics_event = IcsEvent::new(
            format!(
                "{}-{}@matse.morbatex.com",
                event.get_start_date(),
                event.name.to_lowercase().replace(' ', "_")
            ),
            Utc::now().format(DATE_FORMAT).to_string(),
        );
        ics_event.push(DtStart::new(event.get_start_date()));
        if event.is_all_day {
            ics_event.push(Duration::new("P24H"));
        } else {
            ics_event.push(DtEnd::new(event.get_end_date()));
        }
        ics_event.push(Summary::new(escape_text(event.name)));
        if let Some(information) = event.information {
            let information = information.replace("<br />", "\n");
            if !information.is_empty() {
                ics_event.push(Description::new(escape_text(information)));
            }
        }
        if event.location.contains_information() {
            ics_event.push(IcsLocation::new(escape_text(event.location.to_string())));
        }
        if event.lecturer.contains_information() {
            ics_event.push(Organizer::new(escape_text(event.lecturer.to_string())));
        }
        if event.is_lecture {
            ics_event.push(Categories::new("LECTURE"));
        } else if event.is_exercise {
            ics_event.push(Categories::new("Exercise"));
        } else if event.is_holiday {
            ics_event.push(Categories::new("Holiday"));
        }
        ics_event
    }
}

struct Calendar<'a> {
    calendar: ICalendar<'a>,
}

impl<'a> ToString for Calendar<'a> {
    fn to_string(&self) -> String {
        self.calendar.to_string()
    }
}

impl<'a> From<Vec<IcsEvent<'a>>> for Calendar<'a> {
    fn from(events: Vec<IcsEvent<'a>>) -> Self {
        let mut calendar = ICalendar::new("2.0", "-//morbatex/calendar/matse");
        events
            .into_iter()
            .for_each(|event| calendar.add_event(event));
        Self { calendar }
    }
}

impl<'r, 'a: 'r> Responder<'r, 'a> for Calendar<'a> {
    fn respond_to(self, _: &'r rocket::Request<'_>) -> rocket::response::Result<'a> {
        Response::build()
            .header(ContentType::Calendar)
            .header(Header::new(
                CONTENT_DISPOSITION.as_str(),
                " attachment; filename=\"calendar.ics\"",
            ))
            .sized_body(None,Cursor::new(self.to_string()))
            .ok()
    }
}

#[derive(Serialize)]
struct EventCategories {
    name: &'static str,
    curses: HashSet<String>,
}

impl From<(&'static str, HashSet<String>)> for EventCategories {
    fn from((name, curses): (&'static str, HashSet<String>)) -> Self {
        Self { name, curses }
    }
}

async fn get_selected_events<'a>(semester: Semester, curses: Vec<String>) -> Vec<IcsEvent<'a>> {
    get_all_events(semester)
        .await
        .into_iter()
        .filter(|event| curses.contains(&event.name))
        .map(IcsEvent::from)
        .collect::<Vec<_>>()
}

async fn get_all_events(semester: Semester) -> Vec<Event> {
    let mut events = Vec::new();
    for i in 1..=4 {
        events.append(
            &mut get_academic_year_events(semester.clone(), i)
                .await
                .unwrap_or_default(),
        );
    }
    events
}

#[cached(time = 900)] // 900s = 15*60s = 15min
async fn get_academic_year_events(semester: Semester, academic_year: u8) -> Option<Vec<Event>> {
    let url = MATSE_SCHEDULE_URL.join(&academic_year.to_string()).unwrap();
    let query = [
        ("start", semester.get_start_date()?),
        ("end", semester.get_end_date()?),
    ];
    REQWEST_CLIENT
        .get(url)
        .query(&query)
        .send()
        .await
        .ok()?
        .json::<Vec<Event>>()
        .await
        .ok()
}

#[get("/calendar?<winter_semester>&<year>&<curses>")]
async fn get_calendar<'a>(winter_semester: bool, year: i32, curses: Vec<String>) -> Calendar<'a> {
    let semester = Semester {
        year,
        winter_semester,
    };
    let calendar = Calendar::from(get_selected_events(semester, curses).await);
    calendar
}

#[get("/eventCategories?<winter_semester>&<year>")]
async fn get_event_names(winter_semester: bool, year: i32) -> Json<Vec<EventCategories>> {
    let semester = Semester {
        year,
        winter_semester,
    };
    let mut event_names = Vec::new();
    for i in 1..=4 {
        event_names.push(
            (
                ACADEMIC_YEAR_NAMES[i - 1],
                get_academic_year_events(semester.clone(), i as u8)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|event| !event.is_holiday)
                    .map(|event| event.name)
                    .collect(),
            )
                .into(),
        );
    }
    Json(event_names)
}

#[launch]
fn rocket() -> _ {
    rocket::build().mount("/", routes![get_calendar, get_event_names])
}
