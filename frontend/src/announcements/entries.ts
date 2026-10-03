import type { LucideIcon } from 'lucide-react'


/** One feature highlight inside an announcement's expanded view. */
export type AnnouncementFeature = {
  icon: LucideIcon
  /**
   * i18n leaf name: the dialog reads
   * `announcements.entries.<id>.<key>_title` and `…_desc`.
   */
  key: string
}

export type AnnouncementEntry = {
  /** Stable i18n slug: strings live under `announcements.entries.<id>`. */
  id: string
  /**
   * Publish date, ISO `YYYY-MM-DD`. Must be unique and strictly
   * descending down the array — it doubles as the "which announcements
   * has the user seen" ordering.
   */
  date: string
  /**
   * For release announcements: the exact Cargo.toml package version,
   * rendered as a badge on the row. Product news entries omit this.
   * Purely cosmetic — it does not gate visibility. Entries are bundled
   * into the build, so a client that can see an entry is already on the
   * matching release (or newer); there is nothing to hide.
   */
  version?: string
  /** Optional bundled image shown expanded; requires `<id>.image_alt`. */
  image?: string
  /** Optional external action URL; requires `<id>.link_label`. */
  link?: string
  features: AnnouncementFeature[]
}

/**
 * All in-app announcements, newest first. Add an entry (plus locale
 * strings in all four i18n resources) for every release BEFORE tagging
 * it — the release tagging script refuses to tag a version that has no
 * committed entry here. See README.md in this directory for the workflow.
 */
export const ANNOUNCEMENTS: AnnouncementEntry[] = []
