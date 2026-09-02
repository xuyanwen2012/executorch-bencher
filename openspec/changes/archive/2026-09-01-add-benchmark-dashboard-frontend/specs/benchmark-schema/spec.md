## ADDED Requirements

### Requirement: Runs record optional git commit metadata
The system SHALL allow each run to record, in addition to its git commit
SHA and dirty flag, the branch name the run was made from, the commit's
author or committer timestamp as UTC RFC 3339 text, and the commit's
subject line. Each of these three values SHALL be nullable: a run recorded
without them SHALL be accepted, and runs recorded before this metadata
existed SHALL remain readable with null values.

#### Scenario: A run records its commit metadata
- **WHEN** a run is recorded with a branch name, commit timestamp, and
  commit subject
- **THEN** the system stores all three and returns them exactly when the
  run is read back

#### Scenario: A run without commit metadata is accepted
- **WHEN** a run is recorded with no branch, commit timestamp, or subject
- **THEN** the system accepts the write and reads the three values back as
  null

#### Scenario: Existing runs survive the migration
- **WHEN** the migration adding the metadata columns is applied to a
  database that already contains runs
- **THEN** every existing run remains readable, with null commit metadata
