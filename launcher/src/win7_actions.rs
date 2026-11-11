//! Native forms for operations whose modern presentation lives only in GPUI pages.
//! Every network, account and filesystem operation still belongs to its existing core module.
use crate::account::{self, Accounts};
use crate::data_location;
use crate::i18n::tr;
use crate::wardrobe::{self, Change, Saved, Service};
use crate::{backend, shop};
use clap::{Args, Subcommand};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum Action {
    /// List your owned cosmetics, equipped slots, visibility and pending offline changes
    WardrobeList {
        /// Read the last saved answer without accessing the network
        #[arg(long)]
        cached: bool,
    },
    /// Wear an owned item in its catalogue slot; offline changes wait for the service
    WardrobeEquip { slot: String, item: String },
    /// Take an item off a slot; offline changes wait for the service
    WardrobeUnequip { slot: String },
    /// Show or hide your cosmetics to other players
    WardrobeVisible {
        #[arg(value_parser = ["on", "off"])]
        value: String,
    },
    /// Set which other players' cosmetics appear, without changing unrelated game settings
    WardrobePreferences {
        #[arg(long, value_parser = ["shown", "name_tags", "hidden", "automatic"])]
        others: Option<String>,
        #[arg(long, value_parser = ["on", "off"])]
        hide_fx: Option<String>,
    },
    /// Claim the founder set only when the cosmetics service offers this account a free claim
    WardrobeClaim {
        /// You chose to claim the free items for this account
        #[arg(long)]
        confirm: bool,
    },
    /// Read the shop's catalogue, sets, currencies and current availability
    ShopCatalogue,
    /// Read the selected Microsoft account's purchase and refund history
    ShopHistory {
        #[arg(long)]
        cached: bool,
    },
    /// Look up the recipient and preview the basket using the existing shop pricing rules
    ShopQuote {
        #[command(flatten)]
        basket: Basket,
    },
    /// Start checkout on the existing service and show its HTTPS payment URL; no payment is made here
    ShopCheckout {
        #[command(flatten)]
        basket: Basket,
        /// You reviewed the quote and want to open this basket's external checkout
        #[arg(long)]
        confirm: bool,
    },
    /// Start the founder set's free external checkout where the service offers it
    ShopClaim {
        #[arg(long)]
        confirm: bool,
    },
    /// Show the active data folders, fixed location pointer and startup diagnostics
    StorageStatus,
    /// Close the native window and move data using the existing copy-first relocation helper
    StorageMove {
        /// An empty destination folder outside the current launcher, game and build folders
        target: PathBuf,
        /// You checked the destination and chose to move the data and restart
        #[arg(long)]
        confirm: bool,
    },
    /// Retry a repaired data location, then restart the native launcher
    StorageRetry,
    /// Keep the default data folders, retain the old pointer as a backup and restart
    StorageDefault {
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Args)]
pub struct Basket {
    /// An item or set id from Shop Catalogue (repeatable)
    #[arg(long = "item")]
    items: Vec<String>,
    /// A currency offered by the catalogue
    #[arg(long, default_value = "USD")]
    currency: String,
    /// A gift recipient's Minecraft name; otherwise use the selected Microsoft account
    #[arg(long)]
    recipient: Option<String>,
    /// Optional Supporter badge price, in the currency's cents
    #[arg(long)]
    supporter: Option<u32>,
}

fn print(value: Value) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn selected() -> Result<account::Account, String> {
    let accounts = Accounts::load();
    if !matches!(accounts.player(), account::Player::Microsoft(_)) {
        return Err(tr("Sign in to use cosmetics.").into());
    }
    accounts
        .selected()
        .cloned()
        .ok_or_else(|| tr("Sign in to use cosmetics.").into())
}

fn wardrobe_dir() -> PathBuf {
    backend::data_dir().join("wardrobe")
}

fn persist(account: &account::Account, saved: &Saved) -> Result<(), String> {
    saved
        .save(&wardrobe_dir(), &account.uuid)
        .map_err(|e| e.to_string())?;
    let Some(me) = &saved.me else { return Ok(()) };
    let home = backend::game_home();
    horizon_cosmetics::prefs::set_owned(&home, &account.uuid, !me.owned.is_empty())
        .map_err(|e| e.to_string())?;
    if !saved
        .pending
        .iter()
        .any(|c| matches!(c, Change::Visible(_)))
    {
        horizon_cosmetics::prefs::set_confirmed(&home, &account.uuid, me.visible)
            .map_err(|e| e.to_string())?;
        let wish = horizon_cosmetics::prefs::Prefs::load(&home);
        if wish.mine != me.visible {
            horizon_cosmetics::prefs::Prefs {
                mine: me.visible,
                ..wish
            }
            .save(&home)
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn refresh(account: &account::Account, saved: &mut Saved) -> Result<(), wardrobe::Error> {
    let service = Service::live();
    saved.catalogue = Some(service.catalogue()?);
    service.sync_with_session(&account.name, &account.uuid, &mut None, saved, |id| {
        account::session::join_server(&account.uuid, id)
    })
}

fn read_wardrobe(cached: bool, history: bool) -> Result<(), String> {
    let account = selected()?;
    let mut saved = Saved::load(&wardrobe_dir(), &account.uuid);
    let result = if cached {
        Ok(())
    } else {
        refresh(&account, &mut saved)
    };
    persist(&account, &saved)?;
    print(if history {
        json!({"purchases": saved.me.as_ref().map(|me| &me.purchases), "cached": cached,
            "sync_error": result.as_ref().err().map(wardrobe::Error::message)})
    } else {
        json!({"wardrobe": saved, "cached": cached,
            "sync_error": result.as_ref().err().map(wardrobe::Error::message)})
    })?;
    result.map_err(|e| e.message())
}

fn validate_change(saved: &Saved, change: &Change) -> Result<(), String> {
    match change {
        Change::Equip { slot, item } => {
            if !matches!(
                slot.as_str(),
                "head" | "back" | "shoulder" | "name_tag" | "theme"
            ) {
                return Err(wardrobe::Error::NotYours.message());
            }
            if let Some(item) = item {
                let owned = saved.me.as_ref().is_some_and(|me| me.owned.contains(item));
                let fits = saved
                    .catalogue
                    .as_ref()
                    .is_some_and(|c| c.items.iter().any(|e| &e.id == item && &e.slot == slot));
                if !owned || !fits {
                    return Err(wardrobe::Error::NotYours.message());
                }
            }
        }
        Change::Visible(_) => {}
    }
    Ok(())
}

fn change_outfit(change: Change) -> Result<(), String> {
    let account = selected()?;
    let mut saved = Saved::load(&wardrobe_dir(), &account.uuid);
    // Refresh first if the catalogue/ownership has never been cached. A cached outfit can be
    // edited offline, like the modern page; the service still verifies every eventual change.
    if saved.me.is_none() || saved.catalogue.is_none() {
        refresh(&account, &mut saved).map_err(|e| e.message())?;
    }
    validate_change(&saved, &change)?;
    if let Change::Visible(mine) = change {
        let home = backend::game_home();
        horizon_cosmetics::prefs::Prefs {
            mine,
            ..horizon_cosmetics::prefs::Prefs::load(&home)
        }
        .save(&home)
        .map_err(|e| e.to_string())?;
    }
    saved.change(change);
    persist(&account, &saved)?;
    let result = refresh(&account, &mut saved);
    persist(&account, &saved)?;
    print(json!({"wardrobe": saved, "sync_error": result.err().map(|e| e.message())}))
}

fn require_confirmation(confirmed: bool) -> Result<(), String> {
    if confirmed {
        Ok(())
    } else {
        Err(tr("Review this action and select Confirm before running it.").into())
    }
}

fn quote(basket: &Basket) -> Result<(wardrobe::Order, String, Value), String> {
    let service = Service::live();
    let catalogue = service.catalogue().map_err(|e| e.message())?;
    let name = match &basket.recipient {
        Some(name) => name.clone(),
        None => selected()?.name,
    };
    let recipient = service.player(&name).map_err(|e| e.message())?;
    let mut chosen = shop::Basket {
        supporter: basket.supporter,
        ..Default::default()
    };
    for item in &basket.items {
        let exists = catalogue
            .items
            .iter()
            .any(|entry| &entry.id == item && entry.prices.contains_key(&basket.currency))
            || catalogue
                .sets
                .iter()
                .any(|set| &set.id == item && set.prices.contains_key(&basket.currency));
        if !exists {
            return Err(tr("Select items and a currency offered by the shop catalogue.").into());
        }
        if !chosen.items.contains(item) {
            chosen.toggle(item);
        }
    }
    if chosen.is_empty() || !shop::currencies(&catalogue).contains(&basket.currency) {
        return Err(tr("Select items and a currency offered by the shop catalogue.").into());
    }
    if let Some(amount) = basket.supporter {
        let range = catalogue
            .supporter
            .as_ref()
            .and_then(|s| Some((*s.min.get(&basket.currency)?, *s.max.get(&basket.currency)?)));
        if !range.is_some_and(|(min, max)| (min..=max).contains(&amount)) {
            return Err(tr("Select items and a currency offered by the shop catalogue.").into());
        }
    }
    let owned: HashSet<_> = recipient.owned.iter().cloned().collect();
    let (lines, total) = shop::lines(&chosen, &catalogue, &owned, &basket.currency);
    let order = wardrobe::Order {
        name: recipient.name.clone(),
        items: chosen.ids(&catalogue),
        currency: basket.currency.clone(),
        amount: basket.supporter,
        gift: basket.recipient.is_some(),
    };
    let summary = json!({"order": order, "recipient_uuid": recipient.uuid,
        "lines": lines.iter().map(|line| json!({"id": line.id, "amount": line.amount,
            "already_owned": line.owned, "excluded": line.dropped.is_some()})).collect::<Vec<_>>(),
        "estimated_total": total, "formatted_total": shop::format_price(total, &basket.currency),
        "payment_in_external_browser": true});
    Ok((order, recipient.uuid, summary))
}

fn handoff(value: Value) {
    println!("HORIZON_NATIVE_HANDOFF\t{value}");
}

pub fn execute(action: Action) -> Result<(), String> {
    match action {
        Action::WardrobeList { cached } => read_wardrobe(cached, false),
        Action::WardrobeEquip { slot, item } => change_outfit(Change::Equip {
            slot,
            item: Some(item),
        }),
        Action::WardrobeUnequip { slot } => change_outfit(Change::Equip { slot, item: None }),
        Action::WardrobeVisible { value } => change_outfit(Change::Visible(value == "on")),
        Action::WardrobePreferences { others, hide_fx } => {
            let home = backend::game_home();
            let mut prefs = horizon_cosmetics::prefs::Prefs::load(&home);
            let changed = others.is_some() || hide_fx.is_some();
            if let Some(others) = others {
                prefs.others = horizon_cosmetics::prefs::Others::parse(&others);
            }
            if let Some(hide) = hide_fx {
                prefs.hide_fx = hide == "on";
            }
            if changed {
                prefs.save(&home).map_err(|e| e.to_string())?;
            }
            print(
                json!({"mine": prefs.mine, "others": prefs.others.map(|o| o.name()), "hide_fx": prefs.hide_fx}),
            )
        }
        Action::WardrobeClaim { confirm } => {
            require_confirmation(confirm)?;
            let account = selected()?;
            let service = Service::live();
            let session = service
                .sign_in(&account.name, |id| {
                    account::session::join_server(&account.uuid, id)
                })
                .map_err(|e| e.message())?;
            let me = service.me(&session).map_err(|e| e.message())?;
            if !me.claim.here() {
                return Err(wardrobe::Error::Shop("use_checkout".into()).message());
            }
            let mut saved = Saved::load(&wardrobe_dir(), &account.uuid);
            saved.me = Some(service.claim(&session).map_err(|e| e.message())?);
            let result = service.sync(&session, &mut saved);
            persist(&account, &saved)?;
            print(
                json!({"wardrobe": saved, "sync_error": result.as_ref().err().map(wardrobe::Error::message)}),
            )?;
            result.map_err(|e| e.message())
        }
        Action::ShopCatalogue => print(
            serde_json::to_value(Service::live().catalogue().map_err(|e| e.message())?)
                .map_err(|e| e.to_string())?,
        ),
        Action::ShopHistory { cached } => read_wardrobe(cached, true),
        Action::ShopQuote { basket } => print(quote(&basket)?.2),
        Action::ShopCheckout { basket, confirm } => {
            require_confirmation(confirm)?;
            let (order, uuid, summary) = quote(&basket)?;
            if summary["estimated_total"].as_u64() == Some(0) {
                return Err(wardrobe::Error::Shop("owned".into()).message());
            }
            let checkout = Service::live().checkout(&order).map_err(|e| e.message())?;
            if checkout.uuid != uuid {
                return Err(wardrobe::Error::Service(200).message());
            }
            let url = shop::pay_url(&checkout.transaction_id)
                .ok_or_else(|| wardrobe::Error::Service(200).message())?;
            print(json!({"quote": summary, "checkout_url": url,
                "message": tr("Paddle's checkout opens in your browser. Nothing is charged in the launcher.")}))
        }
        Action::ShopClaim { confirm } => {
            require_confirmation(confirm)?;
            let account = selected()?;
            let order = wardrobe::Order {
                name: account.name,
                items: vec![shop::FOUNDER_SET.to_owned()],
                currency: "USD".into(),
                amount: None,
                gift: false,
            };
            let checkout = Service::live().checkout(&order).map_err(|e| e.message())?;
            if checkout.uuid != account.uuid {
                return Err(wardrobe::Error::Service(200).message());
            }
            let url = shop::pay_url(&checkout.transaction_id)
                .ok_or_else(|| wardrobe::Error::Service(200).message())?;
            print(json!({"checkout_url": url,
                "message": tr("Finish the free checkout in your browser. The set lands here a moment later.")}))
        }
        Action::StorageStatus => print(
            json!({"launcher": backend::data_dir(), "game": backend::game_home(),
            "build": backend::build_dir(), "bootstrap": data_location::bootstrap_path()?,
            "configured": data_location::configured_roots()?, "startup_error": data_location::startup_error()}),
        ),
        Action::StorageMove { target, confirm } => {
            require_confirmation(confirm)?;
            if ["HORIZON_HOME", "HORIZON_LAUNCHER_HOME"]
                .iter()
                .any(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()))
            {
                return Err(tr("Data folders are set by HORIZON_LAUNCHER_HOME or HORIZON_HOME. Remove these environment overrides and restart the launcher before moving data here.").into());
            }
            data_location::validate()?;
            handoff(json!({"relocate": target}));
            Ok(())
        }
        Action::StorageRetry => {
            data_location::retry()?;
            handoff(json!({"restart": true}));
            Ok(())
        }
        Action::StorageDefault { confirm } => {
            require_confirmation(confirm)?;
            data_location::use_default_folder()?;
            handoff(json!({"restart": true}));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept_wardrobe() -> Saved {
        serde_json::from_value(json!({
            "catalogue": {"items": [
                {"id":"visor","hash":"123","slot":"head","names":{}},
                {"id":"cape","hash":"456","slot":"back","names":{}}
            ], "claim":{"state":"closed","items":[],"starts_at":null,"ends_at":null}},
            "me":{"uuid":"player","name":"Player","owned":["visor"],"equipped":{},
                "claim":{"state":"closed","items":[],"starts_at":null,"ends_at":null}},
            "pending":[]
        }))
        .unwrap()
    }

    #[test]
    fn cached_equipment_cannot_queue_an_unowned_item_or_a_wrong_slot() {
        let saved = kept_wardrobe();
        assert!(validate_change(
            &saved,
            &Change::Equip {
                slot: "head".into(),
                item: Some("visor".into())
            }
        )
        .is_ok());
        assert!(validate_change(
            &saved,
            &Change::Equip {
                slot: "back".into(),
                item: Some("visor".into())
            }
        )
        .is_err());
        assert!(validate_change(
            &saved,
            &Change::Equip {
                slot: "back".into(),
                item: Some("cape".into())
            }
        )
        .is_err());
        assert!(validate_change(
            &saved,
            &Change::Equip {
                slot: "head".into(),
                item: None
            }
        )
        .is_ok());
    }

    #[test]
    fn checkout_and_storage_changes_refuse_before_network_or_files_without_confirmation() {
        for action in [
            Action::ShopCheckout {
                basket: Basket {
                    items: vec!["visor".into()],
                    currency: "USD".into(),
                    recipient: None,
                    supporter: None,
                },
                confirm: false,
            },
            Action::ShopClaim { confirm: false },
            Action::WardrobeClaim { confirm: false },
            Action::StorageMove {
                target: PathBuf::from("unused destination"),
                confirm: false,
            },
            Action::StorageDefault { confirm: false },
        ] {
            assert_eq!(
                execute(action).unwrap_err(),
                tr("Review this action and select Confirm before running it.")
            );
        }
    }
}
