## ADDED Requirements

### Requirement: Dashboard refreshes live from the event stream
The dashboard SHALL subscribe to the backend's event stream while the
results or runs page is open and SHALL re-fetch the data those pages show
when a `run.created` event arrives, coalescing bursts so that many runs
landing within a short interval cause one refresh. It SHALL show whether
the stream is connected, and when it is not (unsupported, unreachable, or
dropped) SHALL keep working exactly as before with manual reload, retrying
the subscription in the background. A live refresh SHALL NOT discard the
user's filters, paging position, or scroll position.

#### Scenario: A run lands while the results page is open
- **WHEN** a collector posts a run for a configuration currently listed
- **THEN** within a few seconds the row's statistics and counts update
  without the user reloading, and the active filters are unchanged

#### Scenario: A burst of repetitions
- **WHEN** eighteen runs are posted within ten seconds
- **THEN** the page refreshes a small number of times, not eighteen, and
  ends up showing all eighteen

#### Scenario: The stream is unavailable
- **WHEN** the event stream cannot be opened or drops
- **THEN** the page shows a "live updates off" state, all data still loads
  on navigation and reload, and the dashboard reconnects without user
  action once the backend is reachable again
