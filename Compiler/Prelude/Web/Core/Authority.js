// D-AUTHORITY-NAME1=A: web carrier and narrowing operations for the ordinary
// Authority value.
function jet_authority_covers(held, requested) {
  return (
    held === requested ||
    requested.startsWith(`${held}.`) ||
    requested.startsWith(`${held}:`) ||
    requested.startsWith(`${held}/`)
  );
}

function jet_authority_workspace() {
  return { rights: new Set(["FS.Read:repo", "FS.Write:.jet/build"]) };
}

function jet_authority_from_rights(rights) {
  return { rights: new Set(rights.map(right => String(right))) };
}

function jet_authority_with(authority, requested) {
  const right = String(requested);
  if (![...authority.rights].some(held => jet_authority_covers(held, right))) {
    jet_runtime_stop(
      "E0712",
      "<authority>",
      0,
      `E0712: authority cannot narrow to \`${right}\` outside its held rights`,
    );
  }
  return { rights: new Set([right]) };
}

function jet_authority_without(authority, requested) {
  const right = String(requested);
  return {
    rights: new Set(
      [...authority.rights].filter(held => !jet_authority_covers(right, held)),
    ),
  };
}
