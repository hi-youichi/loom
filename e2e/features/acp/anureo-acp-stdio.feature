Feature: anureo acp stdio bridge

  The IDE-facing `anureo acp` command is a stdio JSON-RPC bridge to the
  anureo-server ACP WebSocket endpoint.

  Scenario: initialize through the stdio bridge
    Given a anureo server is running for ACP BDD tests
    And I start the anureo ACP stdio bridge
    When I send an ACP initialize request through stdio
    Then I receive an initialize response with a protocol version

  Scenario: create and restore a session through the stdio bridge
    Given a anureo server is running for ACP BDD tests
    And I start the anureo ACP stdio bridge
    When I create an ACP session through stdio
    And I restart the anureo ACP stdio bridge
    And I load the created session through stdio
    Then the session load request succeeds
