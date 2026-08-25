Feature: anureo CLI ACP client

  The `anureo --acp` mode is a CLI-facing ACP client.

  Scenario: create a session through CLI ACP mode
    Given a anureo server is running for ACP BDD tests
    When I run anureo with --acp and --json
    Then the command exits successfully
    And the output contains a session/new response

  Scenario: resume a session through CLI ACP mode
    Given a session id created by CLI ACP mode
    When I run anureo with --acp and that session id
    Then the command exits successfully
    And the output contains a session/load response

  @requires-provider
  Scenario: execute a prompt through CLI ACP mode
    Given an ACP test provider is configured
    When I run anureo with --acp and a prompt
    Then the command exits successfully
