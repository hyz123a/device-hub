import { expect, type Page, test } from '@playwright/test';

type Device = {
  id: string;
  label: string;
  ssh: null;
  serial: { port: string; baud: number } | null;
  jlink: null;
  adb: null;
};

test('registers and removes a device through the real API', async ({ page, request }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Default Target' })).toBeVisible();
  await expect(page.getByText('board-default', { exact: true })).toBeVisible();
  await pauseForRecording(page);

  await page.getByLabel('Device id').fill('board-smoke');
  await page.getByLabel('Label').fill('Smoke Target');
  await page.getByLabel('Enable serial').check();
  await page.getByPlaceholder('/dev/ttyUSB0').fill('/dev/ttySMOKE0');
  await page.getByRole('button', { name: 'Register device' }).click();

  await expect(page.getByText('Registered Smoke Target (board-smoke)')).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Smoke Target' })).toBeVisible();
  await expect(page.getByText('board-smoke', { exact: true })).toBeVisible();
  await pauseForRecording(page);

  const createdDevices = (await (await request.get('/api/devices')).json()) as Device[];
  expect(createdDevices).toContainEqual({
    id: 'board-smoke',
    label: 'Smoke Target',
    ssh: null,
    serial: { port: '/dev/ttySMOKE0', baud: 115200 },
    jlink: null,
    adb: null,
  });

  page.on('dialog', dialog => dialog.accept());
  await removeDevice(page, 'board-smoke');

  await expect(page.getByText('Removed Smoke Target (board-smoke)')).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Smoke Target' })).toHaveCount(0);
  await pauseForRecording(page);

  await removeDevice(page, 'board-default');

  await expect(page.getByText('Removed Default Target (board-default)')).toBeVisible();
  await expect(page.getByText('No devices registered yet')).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Default Target' })).toHaveCount(0);
  await pauseForRecording(page);

  const remainingDevices = (await (await request.get('/api/devices')).json()) as Device[];
  expect(remainingDevices).toEqual([]);
});

async function removeDevice(page: Page, deviceId: string) {
  const deviceCard = page
    .getByText(deviceId, { exact: true })
    .locator('xpath=ancestor::div[contains(@class, "sm:items-center")][1]');
  await deviceCard.getByRole('button', { name: 'Remove' }).click();
}

async function pauseForRecording(page: Page) {
  const pauseMs = Number(process.env.PLAYWRIGHT_RECORDING_PAUSE_MS ?? 0);
  if (pauseMs > 0) {
    await page.waitForTimeout(pauseMs);
  }
}
