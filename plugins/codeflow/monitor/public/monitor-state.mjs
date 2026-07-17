export function createRequestGate() {
  let current = 0;
  return {
    begin: () => ++current,
    isCurrent: request => request === current,
  };
}

export function selectedSummary(data, runId) {
  if (runId) return data?.runs?.find(run => run.run_id === runId) || null;
  return data?.selected || null;
}
