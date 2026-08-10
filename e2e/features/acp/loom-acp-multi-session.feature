Feature: one Loom server supports multiple ACP sessions

  Background:
    Given one deterministic Loom ACP test server is running
    And an ACP client has initialized protocol version 1

  Scenario: alternate prompts between two sessions
    Given session A uses workspace A
    And session B uses workspace B
    When I prompt session A
    And I prompt session B
    And I prompt session A again
    Then every update is routed to A, B, A

  Scenario: reject overlapping turns in one session
    Given session A is processing a slow prompt
    When I send another prompt to session A
    Then I receive error code -32010

  Scenario: a second connection does not replace the first
    Given connection A owns session A
    And connection B owns session B
    When connection B disconnects
    Then connection A can continue prompting session A

  Scenario: Zed stdio clients share one server
    Given two ACP stdio clients identify as Zed
    When both clients initialize and create sessions
    Then the sessions are distinct and visible through session/list
    And closing one client does not terminate the other client's session

  Scenario: restore cannot change the workspace
    Given session A uses workspace A
    When I load session A with workspace B
    Then I receive an invalid params error

  Scenario: deleting a missing session is idempotent
    When I delete a session id that does not exist
    Then the request succeeds

  Scenario: session metadata survives server restart
    Given session A uses workspace A
    And the Loom server restarts
    When I load session A with workspace A
    Then the request succeeds with the same session id
