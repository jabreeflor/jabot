export interface PrTarget {
  repo: string;
  number: number;
  host?: string;
}
export interface PrAction extends PrTarget {
  action:
    | "comment"
    | "COMMENT"
    | "APPROVE"
    | "REQUEST_CHANGES"
    | "inline"
    | "merge"
    | "close"
    | "reopen"
    | "edit"
    | "reviewers"
    | "ready"
    | "draft";
  title?: string;
  reviewers?: string[];
  body?: string;
  sha?: string;
  method?: "merge" | "squash" | "rebase";
  path?: string;
  line?: number;
  side?: "LEFT" | "RIGHT";
}
export interface PrComment {
  id: number;
  user: { login: string };
  body: string;
  created_at?: string;
  submitted_at?: string;
  state?: string;
  path?: string;
  line?: number;
  html_url: string;
}
export interface PrFile {
  filename: string;
  previous_filename?: string;
  additions: number;
  deletions: number;
  status: string;
  patch?: string;
  blob_url: string;
}
export interface PrWorkspace {
  pr: {
    title: string;
    body: string | null;
    state: string;
    merged: boolean;
    draft: boolean;
    mergeable: boolean | null;
    mergeable_state: string;
    html_url: string;
    user: { login: string };
    head: { sha: string; ref: string };
    base: { ref: string };
    additions: number;
    deletions: number;
    changed_files: number;
    requested_reviewers: { login: string }[];
    labels: { name: string }[];
  };
  comments: PrComment[];
  reviews: PrComment[];
  inline: PrComment[];
  files: PrFile[];
  commits: {
    sha: string;
    html_url: string;
    commit: { message: string; author: { name: string } };
  }[];
  checks: {
    check_runs: {
      id: number;
      name: string;
      status: string;
      conclusion: string | null;
      html_url: string;
    }[];
    total_count: number;
  } | null;
  statuses: {
    state: string;
    statuses: {
      id: number;
      context: string;
      state: string;
      target_url: string | null;
    }[];
    total_count: number;
  } | null;
  checksError: string | null;
}
