use super::PlatformError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivityId(u128);
impl ActivityId {
    pub fn parse(value: &str) -> Result<Self, PlatformError> {
        if value.len() != 32
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(PlatformError::ActivationRejected);
        }
        u128::from_str_radix(value, 16)
            .map(Self)
            .map_err(|_| PlatformError::ActivationRejected)
    }
    pub fn key(self) -> String {
        format!("{:032x}", self.0)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivityAction {
    Open,
    Snooze,
    MarkRead,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivityActivation {
    pub id: ActivityId,
    pub action: ActivityAction,
}
impl ActivityActivation {
    pub fn parse(value: &str) -> Result<Self, PlatformError> {
        let mut parts = value.split('.');
        if parts.next() != Some("activity1") {
            return Err(PlatformError::ActivationRejected);
        }
        let id = ActivityId::parse(parts.next().ok_or(PlatformError::ActivationRejected)?)?;
        let action = match parts.next() {
            Some("open") => ActivityAction::Open,
            Some("snooze") => ActivityAction::Snooze,
            Some("read") => ActivityAction::MarkRead,
            _ => return Err(PlatformError::ActivationRejected),
        };
        if parts.next().is_some() {
            return Err(PlatformError::ActivationRejected);
        }
        Ok(Self { id, action })
    }
    pub fn payload(self) -> String {
        format!(
            "activity1.{}.{}",
            self.id.key(),
            match self.action {
                ActivityAction::Open => "open",
                ActivityAction::Snooze => "snooze",
                ActivityAction::MarkRead => "read",
            }
        )
    }
}
#[derive(Clone, Copy, Debug)]
pub struct ActivityNotification {
    pub id: ActivityId,
    pub events: u32,
    pub sound: bool,
}
impl ActivityNotification {
    #[cfg(any(windows, test))]
    pub(crate) fn xml(self) -> String {
        let action = |op| {
            ActivityActivation {
                id: self.id,
                action: op,
            }
            .payload()
        };
        let detail = if self.events == 1 {
            "1 new activity update".into()
        } else {
            format!("{} new activity updates", self.events)
        };
        let audio = if self.sound {
            ""
        } else {
            "<audio silent='true'/>"
        };
        format!(
            "<toast launch='{open}' activationType='foreground'><visual><binding template='ToastGeneric'><text>Threadline</text><text>{detail}</text></binding></visual>{audio}<actions><action content='Open' arguments='{open}' activationType='foreground'/><action content='Snooze for 1 hour' arguments='{snooze}' activationType='foreground'/><action content='Mark read' arguments='{read}' activationType='foreground'/></actions></toast>",
            open = action(ActivityAction::Open),
            snooze = action(ActivityAction::Snooze),
            read = action(ActivityAction::MarkRead)
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activity_protocol_accepts_only_opaque_local_ids_and_the_three_native_actions() {
        let id = ActivityId::parse("ba2640c0f8e5a316311ad09dc947a5c6").unwrap();
        for action in [
            ActivityAction::Open,
            ActivityAction::MarkRead,
            ActivityAction::Snooze,
        ] {
            let target = ActivityActivation { id, action };
            assert_eq!(ActivityActivation::parse(&target.payload()), Ok(target));
            assert!(ActivityActivation::parse(&(target.payload() + ".other")).is_err());
        }
        assert!(ActivityId::parse("https://private.invalid/path").is_err());
        let xml = ActivityNotification {
            id,
            events: 3,
            sound: false,
        }
        .xml();
        assert!(xml.contains("3 new activity updates"));
        assert!(xml.contains("silent='true'"));
    }
}
