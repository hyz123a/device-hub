import { expect, test } from '@playwright/test';

test('keeps the incremental terminal responsive across sustained websocket batches', async ({
  page,
  request,
}) => {
  const deviceId = 'log-load-target';
  const create = await request.post('/api/devices', {
    data: {
      id: deviceId,
      label: 'Log Load Target',
      ssh: {
        host: '127.0.0.1',
        user: 'root',
        key: '/tmp/device-hub-e2e-key',
        default_log_path: '/var/log/messages',
      },
      serial: null,
      jlink: null,
      adb: null,
    },
  });
  expect(create.ok()).toBeTruthy();

  await page.goto(`/devices/${deviceId}/logs`);
  await expect(page.getByText('connected', { exact: true }).first()).toBeVisible();

  const start = await request.post(`/api/devices/${deviceId}/actions/ssh.exec`, {
    data: { payload: 'generate sustained logs' },
  });
  expect(start.ok()).toBeTruthy();
  const job = (await start.json()) as { id: string; status: string };

  const terminal = page.locator('.xterm-screen');
  await expect(terminal).toContainText('load-line-2000', { timeout: 15_000 });
  await expect(page.getByText(/buffered$/)).toBeVisible();

  await page.getByRole('button', { name: 'Event' }).click();
  await expect(page.getByRole('button', { name: 'Older' })).toBeVisible();
  await page.getByRole('button', { name: 'Stream', exact: true }).click();
  await expect(terminal).toContainText('load-line-2000');

  await expect
    .poll(async () => {
      const response = await request.get(`/api/jobs/${job.id}`);
      return ((await response.json()) as { status: string }).status;
    })
    .toBe('succeeded');

  const jobDetailRequests: URL[] = [];
  page.on('request', pending => {
    const url = new URL(pending.url());
    if (url.pathname.includes(`/jobs/${job.id}/`)) {
      jobDetailRequests.push(url);
    }
  });
  await page.goto(`/jobs/${job.id}`);
  await expect(page.getByRole('heading', { name: 'Evidence summary' })).toBeVisible();
  await expect(page.locator('.xterm-screen')).toContainText('load-line-2000');
  expect(
    jobDetailRequests.filter(url =>
      url.pathname.endsWith(`/jobs/${job.id}/evidence`),
    ),
    'job detail requests evidence metadata once',
  ).toHaveLength(1);
  const historyRequests = jobDetailRequests.filter(url =>
    url.pathname.endsWith(`/jobs/${job.id}/events`),
  );
  expect(
    historyRequests,
    'job detail requests bounded event history once',
  ).toHaveLength(1);
  expect(historyRequests[0].searchParams.get('tail')).toBe('5000');

  const remove = await request.delete(`/api/devices/${deviceId}`);
  expect(remove.ok()).toBeTruthy();
});
