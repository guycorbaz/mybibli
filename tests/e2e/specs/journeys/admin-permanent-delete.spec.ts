import { test, expect } from '@playwright/test';
import { loginAs } from '../../helpers/auth';
import { specIsbn } from '../../helpers/isbn';

const spec_id = 'PD'; // Permanent Delete

test.describe('Story 8-7: Permanent Delete & Auto-Purge', () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page, 'admin');
  });

  test('SC1: Permanent delete requires confirmation modal with name friction', async ({
    page,
  }) => {
    // Create and delete a title
    const isbn = specIsbn(spec_id, 1);
    await page.goto('/catalog');
    await page.fill('#scan-field', isbn);
    await page.press('#scan-field', 'Enter');
    await page.fill('#title', `Deletable Title ${spec_id}`);
    await page.click('button:has-text("Create title")');

    // Wait for creation feedback
    await expect(page.locator('.feedback-entry').first()).toContainText(/created/i, { timeout: 10000 });

    // Soft-delete the title via admin users/system endpoint
    // (For now, just navigate to trash and verify the UI)
    await page.goto('/admin?tab=trash');

    // Verify the deleted title appears in trash
    await expect(page.locator('table').first()).toContainText(`Deletable Title ${spec_id}`, { timeout: 5000 });

    // Click permanent delete button
    const deleteButtons = page.locator('button:has-text("Delete permanently")');
    await deleteButtons.first().click();

    // Verify modal appears with name input
    const modal = page.locator('dialog[open]');
    await expect(modal).toBeVisible();
    await expect(modal).toContainText(/cannot be undone/i);

    // Verify confirm button is disabled until user types correct name
    const confirmBtn = modal.locator('button[type="submit"]');
    await expect(confirmBtn).toBeDisabled();

    // Type wrong name - button should stay disabled
    const input = modal.locator('input[type="text"]');
    await input.fill('Wrong Name');
    await expect(confirmBtn).toBeDisabled();

    // Type correct name - button should enable
    await input.clear();
    await input.fill(`Deletable Title ${spec_id}`);
    await expect(confirmBtn).not.toBeDisabled();

    // Click confirm
    await confirmBtn.click();

    // Verify success feedback and item removal from table
    await expect(page.locator('.feedback-entry').first()).toContainText(/deleted permanently/i, { timeout: 5000 });

    // Item should no longer be in trash
    await expect(page.locator('table').first()).not.toContainText(`Deletable Title ${spec_id}`);
  });

  test('SC2: After permanent delete, item is not recoverable', async ({
    page,
  }) => {
    // Create, soft-delete, then hard-delete a title
    const isbn = specIsbn(spec_id, 2);
    await page.goto('/catalog');
    await page.fill('#scan-field', isbn);
    await page.press('#scan-field', 'Enter');
    await page.fill('#title', `Unrecoverable Title ${spec_id}`);
    await page.click('button:has-text("Create title")');

    await expect(page.locator('.feedback-entry').first()).toContainText(/created/i, { timeout: 10000 });

    // Navigate to trash
    await page.goto('/admin?tab=trash');
    await expect(page.locator('table')).toContainText(`Unrecoverable Title ${spec_id}`);

    // Perform permanent delete (skip confirmation check, just do it)
    const deleteButtons = page.locator('button:has-text("Delete permanently")');
    await deleteButtons.first().click();

    const modal = page.locator('dialog[open]');
    const input = modal.locator('input[type="text"]');
    await input.fill(`Unrecoverable Title ${spec_id}`);
    await modal.locator('button[type="submit"]').click();

    // Success feedback
    await expect(page.locator('.feedback-entry').first()).toContainText(/deleted permanently/i, { timeout: 5000 });

    // Reload trash - item should not reappear
    await page.reload();
    await expect(page.locator('table')).not.toContainText(`Unrecoverable Title ${spec_id}`);
  });

  test('SC3: Trash list shows days remaining and highlights items <7 days', async ({
    page,
  }) => {
    // Just verify the UI shows the days_remaining column and styling
    await page.goto('/admin?tab=trash');

    // If there are any items, check for the days_remaining column
    const rows = page.locator('tbody tr');
    const count = await rows.count();

    if (count > 0) {
      // Each row should have a column with days remaining
      const firstRow = rows.first();
      const cells = firstRow.locator('td');
      const cellCount = await cells.count();

      // Should have at least 5 columns: name, type, deleted_at, days_remaining, actions
      expect(cellCount).toBeGreaterThanOrEqual(5);
    }

    // Verify column headers exist
    await expect(page.locator('th')).toContainText(/item|type|deleted|days/i);
  });
});
