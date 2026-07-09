//! Top-level command dispatch extracted from `cli.rs`.

use std::io;

use clap::CommandFactory;
use clap_complete::{generate, Shell};

use crate::cli::{
    AuthAction, BatchAction, CiAction, CiVarsAction, Cli, Command, CommitAction,
    CommitStatusAction, ConfigAction, DeployAction, DeployEnvAction, DeployEnvVarsAction,
    GlobalArgs, IssueAction, PrAction, RepoAction, SrcAction, StackAction, VariableAction,
    WebhookAction, WorkspaceAction,
};
use crate::commands;
use crate::error::Result;

pub(crate) async fn dispatch(cli: Cli) -> Result<()> {
    let g = &cli.global;
    match cli.command {
        None => {
            let result = commands::status::run_overview(g).await;
            commands::update::notify_if_outdated().await;
            result
        }
        Some(Command::Status {
            g,
            watch,
            interval,
            short,
            export,
        }) => dispatch_status(g, watch, interval, short, export).await,
        Some(Command::Pr { action }) => dispatch_pr(&cli.global, action).await,
        Some(Command::Ci { action }) => dispatch_ci(action).await,
        Some(Command::Repo { action }) => dispatch_repo(action).await,
        Some(Command::Commit { action }) => match action {
            CommitAction::Status { action } => match action {
                CommitStatusAction::Set {
                    commit,
                    key,
                    state,
                    name,
                    url,
                    description,
                    refname,
                    g,
                } => {
                    commands::commit::set_status(
                        &g,
                        commit.as_deref(),
                        &key,
                        &state,
                        name.as_deref(),
                        url.as_deref(),
                        description.as_deref(),
                        refname.as_deref(),
                    )
                    .await
                }
            },
        },
        Some(Command::Batch { action }) => dispatch_batch(&cli.global, action).await,
        Some(Command::Open { action, g }) => commands::open::run(&g, action).await,
        Some(Command::Auth { action }) => dispatch_auth(action).await,
        Some(Command::Completion { shell, install }) => {
            if install {
                commands::completion::install(shell)?;
            } else {
                let shell = shell.unwrap_or(Shell::Bash);
                generate(shell, &mut Cli::command(), "bbr", &mut io::stdout());
            }
            Ok(())
        }
        Some(Command::Config { action }) => match action {
            ConfigAction::Path { g } => commands::config::run_path(&g),
            ConfigAction::Show { g } => commands::config::run_show(&g),
            ConfigAction::Set { key, value, g } => commands::config::run_set(&g, &key, &value),
        },
        Some(Command::Api {
            method,
            path,
            data,
            paginate,
            g,
        }) => commands::api::run(&g, &method, &path, data.as_deref(), paginate).await,
        Some(Command::Webhook { action }) => dispatch_webhook(action).await,
        Some(Command::Src { action }) => match action {
            SrcAction::Cat { path, git_ref, g } => {
                commands::src_cmd::cat(&g, &path, git_ref.as_deref()).await
            }
            SrcAction::Ls { path, git_ref, g } => {
                commands::src_cmd::ls(&g, path.as_deref(), git_ref.as_deref()).await
            }
        },
        Some(Command::Deploy { action }) => dispatch_deploy(action).await,
        Some(Command::Issue { action }) => dispatch_issue(action).await,
        Some(Command::Search {
            query,
            repo,
            limit,
            g,
        }) => commands::search::run(&g, &query, repo.as_deref(), limit).await,
        Some(Command::Schema { model, g }) => commands::schema::run(&g, model.as_deref()),
        Some(Command::Update { check, g }) => commands::update::run(&g, check).await,
        Some(Command::Workspace { action }) => dispatch_workspace(action).await,
        Some(Command::Variable { action }) => dispatch_variable(action).await,
    }
}

async fn dispatch_workspace(action: WorkspaceAction) -> Result<()> {
    match action {
        WorkspaceAction::List { role, limit, g } => {
            commands::workspace::list(&g, role.as_deref(), limit).await
        }
    }
}

async fn dispatch_status(
    g: GlobalArgs,
    watch: bool,
    interval: u64,
    short: bool,
    export: Option<String>,
) -> Result<()> {
    if let Some(fmt) = export {
        let status_res = commands::status::run_inner(&g).await?;
        let text = match fmt.as_str() {
            "slack" => commands::export::format_slack(&status_res),
            "markdown" => commands::export::format_markdown(&status_res),
            _ => unreachable!(),
        };
        crate::output::print_block(&text)?;
        Ok(())
    } else if watch {
        commands::status::run_watch(&g, interval).await
    } else if short {
        commands::status::run_short(&g).await
    } else {
        commands::status::run(&g).await
    }
}

async fn dispatch_pr(g: &GlobalArgs, action: PrAction) -> Result<()> {
    match action {
        PrAction::List {
            state,
            limit,
            author,
            source_branch,
            reviewer,
            sort,
            order,
            g,
        } => {
            commands::pr::list(
                &g,
                &state,
                limit,
                author.as_deref(),
                source_branch.as_deref(),
                reviewer.as_deref(),
                &sort,
                &order,
            )
            .await
        }
        PrAction::View {
            id,
            diff,
            comments,
            g,
        } => commands::pr::view(&g, id, diff, comments).await,
        PrAction::Create {
            title,
            body,
            body_file,
            body_stdin,
            src,
            dst,
            close_source_branch,
            draft,
            reviewer,
            g,
        } => {
            commands::pr::create(
                &g,
                &title,
                body.as_deref(),
                body_file.as_deref(),
                body_stdin,
                src.as_deref(),
                dst.as_deref(),
                close_source_branch,
                draft,
                &reviewer,
            )
            .await
        }
        PrAction::Comment {
            id,
            body,
            body_file,
            body_stdin,
            reply_to,
            g,
        } => {
            commands::pr::comment(
                &g,
                id,
                body.as_deref(),
                body_file.as_deref(),
                body_stdin,
                reply_to,
            )
            .await
        }
        PrAction::Comments { id, limit, g } => commands::pr::comments(&g, id, limit).await,
        PrAction::Tasks { id, limit, g } => commands::pr::tasks(&g, id, limit).await,
        PrAction::Commits { id, limit, g } => commands::pr::commits(&g, id, limit).await,
        PrAction::Statuses { id, limit, g } => commands::pr::statuses(&g, id, limit).await,
        PrAction::Conflicts { id, limit, g } => commands::pr::conflicts(&g, id, limit).await,
        PrAction::RequestChanges { id, g } => commands::pr::request_changes(&g, id).await,
        PrAction::UnrequestChanges { id, g } => commands::pr::unrequest_changes(&g, id).await,
        PrAction::Merge {
            id,
            close_source_branch,
            strategy,
            message,
            g,
        } => {
            commands::pr::merge(
                &g,
                id,
                close_source_branch,
                strategy.as_deref(),
                message.as_deref(),
            )
            .await
        }
        PrAction::Approve { id, message, g } => {
            commands::pr::approve(&g, id, message.as_deref()).await
        }
        PrAction::Unapprove { id, g } => commands::pr::unapprove(&g, id).await,
        PrAction::Decline { id, g } => commands::pr::decline(&g, id).await,
        PrAction::Checkout { id, g } => commands::pr::checkout(&g, id).await,
        PrAction::Diff {
            id,
            raw,
            side_by_side,
            context,
            no_syntax,
            g,
        } => commands::pr::diff(&g, id, raw, side_by_side, context, no_syntax).await,
        PrAction::Diffstat { id, g } => commands::pr::diffstat(&g, id).await,
        PrAction::Patch { id, output, g } => commands::pr::patch(&g, id, output.as_deref()).await,
        PrAction::Update {
            id,
            title,
            description,
            g,
        } => commands::pr::update(&g, id, title.as_deref(), description.as_deref()).await,
        PrAction::Dashboard { repos, filter, g } => {
            commands::dashboard::run_dashboard(&g, repos, filter.as_deref()).await
        }
        PrAction::Stack { action } => match action {
            StackAction::Init { name, base } => commands::stack::init(g, &name, base.as_deref()),
            StackAction::Add { branch, parent } => {
                commands::stack::add(g, &branch, parent.as_deref()).await
            }
            StackAction::List => commands::stack::list(g).await,
            StackAction::Rebase { push } => commands::stack::rebase(g, push).await,
            StackAction::Land { strategy, yes } => {
                commands::stack::land(g, strategy.as_deref(), yes).await
            }
            StackAction::Abort { yes } => commands::stack::abort(g, yes).await,
        },
    }
}

async fn dispatch_ci(action: CiAction) -> Result<()> {
    match action {
        CiAction::Status { branch, g } => commands::ci::status(&g, branch.as_deref()).await,
        CiAction::Watch {
            branch,
            interval,
            logs,
            g,
        } => commands::ci::watch(&g, branch.as_deref(), interval, logs).await,
        CiAction::Logs {
            uuid,
            step,
            failed,
            latest,
            output,
            g,
        } => {
            commands::ci::logs(
                &g,
                uuid.as_deref(),
                step.as_deref(),
                failed,
                latest,
                output.as_deref(),
            )
            .await
        }
        CiAction::List { branch, limit, g } => {
            commands::ci::list(&g, branch.as_deref(), limit).await
        }
        CiAction::Rerun { branch, g } => commands::ci::rerun(&g, branch.as_deref()).await,
        CiAction::Trigger {
            branch,
            vars,
            secured,
            g,
        } => commands::ci::trigger(&g, branch.as_deref(), &vars, &secured).await,
        CiAction::Stop { uuid, branch, g } => {
            commands::ci::stop(&g, uuid.as_deref(), branch.as_deref()).await
        }
        CiAction::Steps { uuid, g } => commands::ci::steps(&g, uuid.as_deref()).await,
        CiAction::Tests {
            uuid,
            step,
            limit,
            g,
        } => commands::ci::tests(&g, uuid.as_deref(), step.as_deref(), limit).await,
        CiAction::Compare { a, b, g } => commands::ci_compare::compare(&g, &a, &b).await,
        CiAction::Vars { action } => match action {
            CiVarsAction::List { g } => commands::ci_vars::list(&g).await,
            CiVarsAction::Set {
                key,
                value,
                secured,
                g,
            } => commands::ci_vars::set(&g, &key, &value, secured).await,
            CiVarsAction::Delete { key, g } => commands::ci_vars::delete(&g, &key).await,
        },
    }
}

async fn dispatch_repo(action: RepoAction) -> Result<()> {
    match action {
        RepoAction::Info { g } => commands::repo::info(&g).await,
        RepoAction::Branches { limit, g } => commands::repo::list_branches(&g, limit).await,
        RepoAction::Tags { limit, g } => commands::repo::list_tags(&g, limit).await,
        RepoAction::Commits { branch, limit, g } => {
            commands::repo::list_commits(&g, branch.as_deref(), limit).await
        }
        RepoAction::Create {
            slug,
            private,
            description,
            language,
            enable_issues,
            g,
        } => {
            commands::repo::create(
                &g,
                &slug,
                private,
                description.as_deref(),
                language.as_deref(),
                enable_issues,
            )
            .await
        }
        RepoAction::Audit { slug, g } => commands::audit::run_audit(&g, slug.as_deref()).await,
        RepoAction::Delete { slug, yes, g } => commands::repo::delete(&g, &slug, yes).await,
        RepoAction::Fork {
            slug,
            name,
            target_workspace,
            g,
        } => {
            commands::repo::fork(
                &g,
                slug.as_deref(),
                name.as_deref(),
                target_workspace.as_deref(),
            )
            .await
        }
        RepoAction::CreateBranch { name, from, g } => {
            commands::repo::create_branch(&g, &name, from.as_deref()).await
        }
        RepoAction::CreateTag {
            name,
            target,
            message,
            g,
        } => commands::repo::create_tag(&g, &name, target.as_deref(), message.as_deref()).await,
        RepoAction::Permissions { g } => commands::repo::permissions(&g).await,
    }
}

async fn dispatch_batch(g: &GlobalArgs, action: BatchAction) -> Result<()> {
    match action {
        BatchAction::MergeApproved {
            repo,
            dry_run,
            strategy,
            yes,
            max,
        } => {
            commands::batch::merge_approved(
                g,
                repo.as_deref(),
                dry_run,
                strategy.as_deref(),
                yes,
                max,
            )
            .await
        }
        BatchAction::RerunFailed {
            branch,
            repo,
            dry_run,
            yes,
            max,
        } => {
            commands::batch::rerun_failed(g, branch.as_deref(), repo.as_deref(), dry_run, yes, max)
                .await
        }
        BatchAction::CleanupMergedBranches {
            repo,
            remote,
            dry_run,
            yes,
            max,
        } => {
            commands::batch::cleanup_merged_branches(g, repo.as_deref(), remote, dry_run, yes, max)
                .await
        }
    }
}

async fn dispatch_auth(action: AuthAction) -> Result<()> {
    match action {
        AuthAction::Setup { username, token } => commands::auth::setup(username, token),
        AuthAction::Status { g } => commands::auth::status(&g).await,
        AuthAction::Logout { g } => commands::auth::logout(&g),
        AuthAction::Test { g } => commands::auth::test(&g).await,
    }
}

async fn dispatch_webhook(action: WebhookAction) -> Result<()> {
    match action {
        WebhookAction::List { g } => commands::webhook::list(&g).await,
        WebhookAction::View { uid, g } => commands::webhook::view(&g, &uid).await,
        WebhookAction::Create {
            url,
            events,
            description,
            active,
            secret,
            g,
        } => {
            commands::webhook::create(
                &g,
                &url,
                &events,
                description.as_deref(),
                active,
                secret.as_deref(),
            )
            .await
        }
        WebhookAction::Update {
            uid,
            url,
            events,
            description,
            active,
            g,
        } => {
            commands::webhook::update(
                &g,
                &uid,
                url.as_deref(),
                events.as_deref(),
                description.as_deref(),
                active,
            )
            .await
        }
        WebhookAction::Delete { uid, yes, g } => commands::webhook::delete(&g, &uid, yes).await,
    }
}

async fn dispatch_deploy(action: DeployAction) -> Result<()> {
    match action {
        DeployAction::List { limit, g } => commands::deploy::list_deployments(&g, limit).await,
        DeployAction::Trigger {
            env_uuid,
            commit,
            g,
        } => commands::deploy::trigger_deployment(&g, &env_uuid, &commit).await,
        DeployAction::Env { action } => match action {
            DeployEnvAction::List { g } => commands::deploy::list_environments(&g).await,
            DeployEnvAction::Create { name, env_type, g } => {
                commands::deploy::create_environment(&g, &name, &env_type).await
            }
            DeployEnvAction::Vars { action } => match action {
                DeployEnvVarsAction::List { env_uuid, g } => {
                    commands::deploy::list_env_vars(&g, &env_uuid).await
                }
                DeployEnvVarsAction::Set {
                    env_uuid,
                    key,
                    value,
                    secured,
                    g,
                } => commands::deploy::set_env_var(&g, &env_uuid, &key, &value, secured).await,
                DeployEnvVarsAction::Delete { env_uuid, key, g } => {
                    commands::deploy::delete_env_var(&g, &env_uuid, &key).await
                }
            },
        },
    }
}

async fn dispatch_issue(action: IssueAction) -> Result<()> {
    match action {
        IssueAction::List {
            limit,
            status,
            kind,
            priority,
            assignee,
            query,
            g,
        } => {
            commands::issue::list(
                &g,
                limit,
                status.as_deref(),
                kind.as_deref(),
                priority.as_deref(),
                assignee.as_deref(),
                query.as_deref(),
            )
            .await
        }
        IssueAction::View { id, comments, g } => commands::issue::view(&g, id, comments).await,
        IssueAction::Create {
            title,
            body,
            kind,
            priority,
            assignee,
            g,
        } => {
            commands::issue::create(&g, &title, &body, &kind, &priority, assignee.as_deref()).await
        }
        IssueAction::Update {
            id,
            title,
            body,
            status,
            kind,
            priority,
            assignee,
            g,
        } => {
            commands::issue::update(
                &g,
                id,
                title.as_deref(),
                body.as_deref(),
                status.as_deref(),
                kind.as_deref(),
                priority.as_deref(),
                assignee.as_deref(),
            )
            .await
        }
        IssueAction::Comment { id, body, g } => commands::issue::comment(&g, id, &body).await,
        IssueAction::Comments { id, limit, g } => {
            commands::issue::list_comments(&g, id, limit).await
        }
    }
}

async fn dispatch_variable(action: VariableAction) -> Result<()> {
    match action {
        VariableAction::List { g } => commands::ci_vars::list(&g).await,
        VariableAction::Set {
            key,
            value,
            secured,
            g,
        } => commands::ci_vars::set(&g, &key, &value, secured).await,
        VariableAction::Delete { key, g } => commands::ci_vars::delete(&g, &key).await,
    }
}
