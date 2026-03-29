import { Page } from "@playwright/test";

/**
 * Log in as the specified role.
 */
export async function loginAs(
  page: Page,
  role: "admin" | "librarian",
): Promise<void> {
  // Stub — will be implemented when authentication is built
}

/**
 * Log out the current user.
 */
export async function logout(page: Page): Promise<void> {
  // Stub — will be implemented when authentication is built
}
