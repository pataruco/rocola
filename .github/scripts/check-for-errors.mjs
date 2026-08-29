/**
 * Fails the workflow if any upstream job in the `needs` context
 * finished as `failure` or `cancelled`.
 *
 * @param {{ core: import('@actions/core'), jobs: Record<string, { result: string }> }} args
 */
export default function checkJobsForErrors({ core, jobs }) {
  const entries = Object.entries(jobs);

  if (entries.length === 0) {
    core.warning('No upstream jobs found in the needs context.');
    return;
  }

  const failed = entries.filter(([, { result }]) =>
    result === 'failure' || result === 'cancelled',
  );

  for (const [name, { result }] of entries) {
    core.info(`${name}: ${result}`);
  }

  if (failed.length > 0) {
    core.setFailed(
      `Upstream job(s) did not succeed: ${failed
        .map(([name, { result }]) => `${name} (${result})`)
        .join(', ')}`,
    );
  }
}
