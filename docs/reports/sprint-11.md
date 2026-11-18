# Sprint 11 activity and Windows notifications

The native Windows fallback journey passed with the same executable copied outside its registered location. All 37 synthetic mentions reached the in-app feed. Through the window's own accessibility controls, the tester snoozed the target for one hour and marked one event read. The original issue 41 tab and its draft survived. The test archived the synthetic profile after a normal exit. [Native receipt](evidence/sprint-11/windows-delivery.json)

| Windows case | Result | What ran |
| --- | --- | --- |
| UT-014 and UT-015 | Pass | Category, mention, actor, quiet-hour and snooze rules. Event deduplication and grouping by exact target. |
| IT-018 | Pass | Three real SQLite and observer tests covered repeated observations, changed preferences, catch-up grouping and recovery after an uncertain delivery. |
| E2E-018 | Pass | A guarded Windows 11 x64 process used the normal in-app feed while its copied executable lacked the installed sender registration. |
| E2E-012, first installed attempt | Blocked | The original installed process published all 37 events to the feed and reserved zero native groups. The driver never obtained a notification to open after process exit. |

The first two rows ran on Windows at `8a22b8d`. Six native test executables passed 48 tests in total. The native desktop journeys used clean runner `dd0f6a4` with seed 12345, Windows 11 build 22631.7652 and local package 9. The app was built at `1ddd9c8`. Use `cargo xtask test e2e --suite full --case E2E-018 --local-fixture <fixture.json>` or select `E2E-012` for the registered path. [Windows test binaries and results](evidence/sprint-11/windows-storage.json) · [Native fixture and driver](../../tools/windows/test-installed-activity.ps1)

The first installed build recorded the generic operating-system fallback reason. A follow-up probe verified the exact registered command, shortcut target, AppUserModelID and activator. Windows created the scoped notifier. Reading its per-sender setting returned `0x80070490` (element not found) both before and after the probe assigned its explicit process AppUserModelID. The Windows response contained no permission decision. [Cold attempt and sender probe](evidence/sprint-11/windows-delivery.json) · [Microsoft's notification setting reference](https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotifier.setting)

The scheduling independence case and the published Windows package retain their own gates. [Windows scenario ledger](../spec/test_platform_traceability.csv) · [Sprint requirements](../spec/sprints/sprint_11_notifications.md)
