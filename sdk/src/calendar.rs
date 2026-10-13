// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Calendars: the person's and their shared mailboxes', events in a
//! range with recurring ones expanded, changes of one occurrence, the
//! following ones or all, and invitations answered. Calendar apps use
//! CalDAV instead.

use crate::client::{seg, Repeat};
use crate::error::Error;
use crate::types::*;
use crate::Client;

impl Client {
    /// The calendars the person sees, their own first. A default one is
    /// made the first time they are asked for.
    pub fn calendars(&self) -> Result<CalendarList, Error> {
        self.get("/api/calendars", &[])
    }

    /// Every occurrence from `from` to `to` (RFC 3339, at most 400 days
    /// apart), recurring events expanded in the zone of their start, in
    /// the calendars named, or in every one when `calendars` is empty.
    /// Sorted by start, then by summary.
    pub fn occurrences(&self, from: &str, to: &str, calendars: &[&str]) -> Result<Vec<EventView>, Error> {
        #[derive(serde::Deserialize)]
        struct List {
            #[serde(default)]
            events: Vec<EventView>,
        }
        let ids = calendars.join(",");
        let mut q = vec![("from", from), ("to", to)];
        if !ids.is_empty() {
            q.push(("calendars", ids.as_str()));
        }
        let mut v: List = self.get("/api/calendars/occurrences", &q)?;
        // RFC 3339 in UTC, as the service writes it, sorts as text.
        v.events
            .sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.summary.cmp(&b.summary)));
        Ok(v.events)
    }

    /// An event with its description, attendees, reminders and iCalendar
    /// text: the occurrence `recurrence_id` names, or the first.
    pub fn event(&self, calendar: &str, event: &str, recurrence_id: &str) -> Result<EventDetail, Error> {
        let q: Vec<(&str, &str)> = if recurrence_id.is_empty() {
            vec![]
        } else {
            vec![("recurrenceId", recurrence_id)]
        };
        self.get(
            &format!("/api/calendars/{}/events/{}", seg(calendar), seg(event)),
            &q,
        )
    }

    /// Makes an event. With attendees it is the person's (or the shared
    /// mailbox's, when they may send as it), and each attendee is sent an
    /// invitation, unless `notify` is false.
    pub fn create_event(
        &self,
        calendar: &str,
        event: &EventInput,
        notify: bool,
    ) -> Result<EventChange, Error> {
        let mut body = serde_json::json!({"event": event});
        if !notify {
            body["notify"] = false.into();
        }
        self.send(
            "POST",
            &format!("/api/calendars/{}/events", seg(calendar)),
            &body,
            Repeat::Once,
        )
    }

    /// Changes an event: one occurrence, the following ones or all. An
    /// attendee changes only their reminders and whether the time shows
    /// as busy (`403 not_organizer` otherwise). The attendees are sent
    /// an update, and those removed a cancellation, unless `notify` is
    /// false.
    pub fn update_event(
        &self,
        calendar: &str,
        event: &str,
        change: &EventUpdate,
    ) -> Result<EventChange, Error> {
        self.send(
            "PATCH",
            &format!("/api/calendars/{}/events/{}", seg(calendar), seg(event)),
            change,
            Repeat::Once,
        )
    }

    /// Deletes an event, or with `scope` and `recurrence_id` some of its
    /// occurrences. A non-empty `etag` is refused with `412 changed` when
    /// the event changed meanwhile. The attendees of an event the person
    /// organises are sent a cancellation, unless `notify` is false.
    pub fn delete_event(
        &self,
        calendar: &str,
        event: &str,
        scope: Option<Scope>,
        recurrence_id: &str,
        etag: &str,
        notify: bool,
    ) -> Result<EventChange, Error> {
        let mut q: Vec<(&str, &str)> = Vec::new();
        if let Some(s) = scope {
            q.push(("scope", s.as_str()));
        }
        if !recurrence_id.is_empty() {
            q.push(("recurrenceId", recurrence_id));
        }
        if !etag.is_empty() {
            q.push(("etag", etag));
        }
        if !notify {
            q.push(("notify", "false"));
        }
        let url = self.url(
            &format!("/api/calendars/{}/events/{}", seg(calendar), seg(event)),
            &q,
        );
        let resp = self.exchange("DELETE", &url, None, &[], None, Repeat::Once)?;
        Ok(self.decode(resp)?.0)
    }

    /// Answers an invitation the calendar holds, and sends the organizer
    /// the answer.
    pub fn respond_event(
        &self,
        calendar: &str,
        event: &str,
        response: Response,
        comment: &str,
    ) -> Result<Answered, Error> {
        self.send(
            "POST",
            &format!("/api/calendars/{}/events/{}/respond", seg(calendar), seg(event)),
            &answer(response, "", comment),
            Repeat::Once,
        )
    }

    /// The invitation a message carries: what, when, the account's
    /// answer, whether the calendar has this version, and the account's
    /// own events at the same time. `no_invitation` when it carries none.
    pub fn invitation(&self, account: &str, message: &str) -> Result<Invitation, Error> {
        #[derive(serde::Deserialize)]
        struct One {
            invitation: Invitation,
        }
        let v: One = self.get(
            &format!(
                "/api/accounts/{}/messages/{}/invitation",
                seg(account),
                seg(message)
            ),
            &[],
        )?;
        Ok(v.invitation)
    }

    /// Answers the invitation a message carries: the event goes into the
    /// calendar (`calendar`, or the account's default when empty) and the
    /// organizer is sent the answer, in one step.
    pub fn answer_invitation(
        &self,
        account: &str,
        message: &str,
        response: Response,
        calendar: &str,
        comment: &str,
    ) -> Result<Answered, Error> {
        self.send(
            "POST",
            &format!(
                "/api/accounts/{}/messages/{}/invitation",
                seg(account),
                seg(message)
            ),
            &answer(response, calendar, comment),
            Repeat::Once,
        )
    }
}

fn answer(response: Response, calendar: &str, comment: &str) -> serde_json::Value {
    let mut v = serde_json::json!({"response": response.as_str()});
    if !calendar.is_empty() {
        v["calendarId"] = calendar.into();
    }
    if !comment.is_empty() {
        v["comment"] = comment.into();
    }
    v
}
