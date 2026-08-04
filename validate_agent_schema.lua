meta = {
  reasoning = "Validate that the agent respects and produces structured output matching the provided JSON schema",
  phases = {
    { label = "validate", description = "Test agent schema validation with a structured output schema" },
    { label = "report" },
  },
}

local PERSON_SCHEMA = {
  type = "object",
  properties = {
    name = { type = "string" },
    age = { type = "integer" },
    email = { type = "string" },
    role = { type = "string", enum = { "engineer", "designer", "manager" } }
  },
  required = { "name", "age", "email", "role" }
}

function main()
  phase("validate", 1)

  local result = agent({
    prompt = "You are a profile generator. Create a fictional team member profile with name, age, email, and role (one of: engineer, designer, manager). "
          .. "Return ONLY valid JSON matching the schema.",
    schema = PERSON_SCHEMA,
    name = "schema-validator",
    description = "Generate a structured profile to validate schema enforcement"
  })

  phase("report")
  if not result.ok then
    report({ error = "Schema validation failed: " .. result.status, result = result })
    return
  end

  local output = result.output
  local validations = {
    name_is_string = type(output.name) == "string",
    age_is_integer = type(output.age) == "number" and math.floor(output.age) == output.age,
    email_is_string = type(output.email) == "string",
    role_is_valid = output.role == "engineer" or output.role == "designer" or output.role == "manager"
  }

  local all_valid = true
  for k, v in pairs(validations) do
    if not v then
      all_valid = false
      log("Validation failed: " .. k, "warn")
    end
  end

  report({
    schema_valid = all_valid,
    validations = validations,
    profile = output,
    tokens_used = result.tokens,
    status = result.status
  })
end