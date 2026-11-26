use crate::card::Card;
use crate::consumable::Consumable;
use crate::deck::Deck;
use crate::joker::Jokers;
use crate::pack::{Pack, PackCategory, PackContent, PackSize};
use crate::planet::{Planetarium, Planets};
use crate::shop::{
    gen_random_playing_card, gen_shop_playing_card, ConsumableGenerator, JokerGenerator,
    PackGenerator, ShopContext,
};
use crate::spectral::{random_spectral, Spectral};
use crate::tag::Tag;
use crate::tarot::Tarot;
use crate::voucher::{Voucher, Vouchers};
use balatro_seed::Instance;
use balatro_types::{BossBlind, Edition, Rarity, Suit, Value};
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use strum::IntoEnumIterator;

/// What a single shop-item generation call produced. The category split
/// has to happen inside the backend, since `Real` mode's category roll is
/// bundled into one `Instance::next_shop_item` call.
pub(crate) enum GeneratedItem {
    Joker(Jokers),
    Consumable(Consumable),
    /// Only ever produced with the Magic Trick voucher redeemed.
    PlayingCard(Card),
}

pub(crate) trait RngBackend {
    fn gen_shop_item(
        &mut self,
        ctx: &ShopContext,
        exclude_jokers: &[Jokers],
        exclude_tarots: &[Tarot],
        exclude_planets: &[Planets],
    ) -> GeneratedItem;

    fn gen_pack(&mut self, ctx: &ShopContext, exclude: Option<(&PackCategory, &PackSize)>) -> Pack;

    /// This ante's voucher offer, drawn from the vouchers still
    /// offerable (see `Vouchers::offerable`). `None` once all 32 are
    /// redeemed.
    fn gen_voucher(&mut self, ante: i32, vouchers: &Vouchers) -> Option<Voucher>;

    /// Owned-joker dedup hook: called on buy/sell so `Real` mode's lock
    /// table stays accurate. No-op for `Fast` mode.
    fn on_joker_bought(&mut self, joker: &Jokers);
    fn on_joker_sold(&mut self, joker: &Jokers);
    /// Redeemed-voucher hook: `Real` mode's draw rates read straight off
    /// its own active-voucher list, so it has to be told. No-op for
    /// `Fast` mode, which reads `ShopContext::vouchers` per call instead.
    fn on_voucher_bought(&mut self, voucher: &Voucher);
    /// Jokers::Showman's real effect. No call site yet — see `jokers.md`.
    #[allow(dead_code)]
    fn set_showman(&mut self, owned: bool);

    /// A single freshly generated joker, outside the shop/pack context —
    /// currently only Judgement. Distinct from `gen_shop_item`'s joker arm
    /// since a shop slot's category (joker vs. consumable) is its own roll.
    fn gen_joker(&mut self, ante: i32, prob_mult: u32, exclude: &[Jokers]) -> Jokers;
    fn gen_joker_of_rarity(
        &mut self,
        _ante: i32,
        prob_mult: u32,
        exclude: &[Jokers],
        rarity: Rarity,
    ) -> Jokers;
    fn pick_random_joker(&mut self, available: Vec<Jokers>) -> Jokers;
    fn clone_joker(&mut self, j: Jokers) -> Jokers;
    fn shuffle_deck(&mut self, deck: &mut Deck);
    fn prob_roll(&mut self, numerator: u32, denominator: u32) -> bool;
    /// Returns `(small_blind_tag, big_blind_tag)` for a fresh ante.
    fn draw_ante_tags(&mut self) -> (Tag, Tag);
    /// Returns the Boss Blind for a fresh ante.
    fn draw_boss(&mut self, ante: i32) -> BossBlind;
    /// A single random Tarot consumable, respecting `exclude` — used by
    /// the purple-seal discard trigger and the Emperor/High Priestess
    /// tarot effects.
    fn roll_random_tarot(&mut self, exclude: &[Tarot]) -> Consumable;
    /// A single random Planet consumable, respecting `exclude` — used by
    /// the blue-seal round-end trigger and the High Priestess tarot effect.
    fn roll_random_planet(&mut self, planetarium: &Planetarium, exclude: &[Planets]) -> Consumable;
    fn roll_random_spectral(&mut self) -> Spectral;
    fn roll_int(&mut self, min: u32, max: u32) -> u32;
    /// Castle/MailInRebate's per-round reroll (`Game::clear_blind`). Mint-time
    /// rolling for freshly generated jokers already happens inside
    /// `gen_joker`/`gen_shop_item`/`gen_pack`, each already using the right
    /// stream per backend — this is only for rerolling an *existing* joker.
    fn roll_discard_selector(&mut self, j: &mut Jokers);
    fn roll_random_edition(&mut self) -> Edition;
    fn roll_random_suit(&mut self) -> Suit;
    fn roll_random_value(&mut self) -> Value;
    fn pick_random_card(&mut self, available: Vec<Card>) -> Card;
    fn gen_random_card(
        &mut self,
        prob_mult: u32,
        force_enhance: bool,
        force_values: Option<&[Value]>,
    ) -> Card;
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub(crate) struct FastBackend {
    rng: ChaCha8Rng,
    joker_gen: JokerGenerator,
    consumable_gen: ConsumableGenerator,
    pack_gen: PackGenerator,
}

impl FastBackend {
    pub(crate) fn new(rng: ChaCha8Rng) -> Self {
        FastBackend {
            rng,
            joker_gen: JokerGenerator::new(),
            consumable_gen: ConsumableGenerator::new(),
            pack_gen: PackGenerator {},
        }
    }
}

impl RngBackend for FastBackend {
    fn gen_shop_item(
        &mut self,
        ctx: &ShopContext,
        exclude_jokers: &[Jokers],
        exclude_tarots: &[Tarot],
        exclude_planets: &[Planets],
    ) -> GeneratedItem {
        // Joker=20, Tarot/Planet=4 each before the Merchant/Tycoon
        // vouchers raise them; playing cards weigh 0 until Magic Trick.
        let weights = [
            20.0,
            ctx.vouchers.tarot_rate(),
            ctx.vouchers.planet_rate(),
            ctx.vouchers.playing_card_rate(),
        ];
        let dist = WeightedIndex::new(weights).expect("shop item weights are finite and nonzero");
        match dist.sample(&mut self.rng) {
            0 => GeneratedItem::Joker(self.joker_gen.gen_joker(
                ctx.edition_prob_mult(),
                exclude_jokers,
                &mut self.rng,
            )),
            1 => GeneratedItem::Consumable(
                self.consumable_gen
                    .gen_tarot_consumable(exclude_tarots, &mut self.rng),
            ),
            2 => GeneratedItem::Consumable(self.consumable_gen.gen_planet_consumable(
                ctx.planetarium,
                exclude_planets,
                &mut self.rng,
            )),
            _ => GeneratedItem::PlayingCard(gen_shop_playing_card(
                ctx.edition_prob_mult(),
                ctx.vouchers.shop_cards_are_modified(),
                &mut self.rng,
            )),
        }
    }

    fn gen_pack(&mut self, ctx: &ShopContext, exclude: Option<(&PackCategory, &PackSize)>) -> Pack {
        self.pack_gen.gen_pack(
            ctx.planetarium,
            ctx.edition_prob_mult(),
            exclude,
            ctx.held_jokers,
            &mut self.rng,
        )
    }

    fn gen_voucher(&mut self, _ante: i32, vouchers: &Vouchers) -> Option<Voucher> {
        let offerable = vouchers.offerable();
        if offerable.is_empty() {
            return None;
        }
        Some(offerable[self.rng.gen_range(0..offerable.len())])
    }

    fn on_joker_bought(&mut self, _joker: &Jokers) {}
    fn on_joker_sold(&mut self, _joker: &Jokers) {}
    fn on_voucher_bought(&mut self, _voucher: &Voucher) {}
    fn set_showman(&mut self, owned: bool) {
        self.joker_gen.showman = owned;
    }

    fn gen_joker(&mut self, _ante: i32, prob_mult: u32, exclude: &[Jokers]) -> Jokers {
        self.joker_gen.gen_joker(prob_mult, exclude, &mut self.rng)
    }

    fn gen_joker_of_rarity(
        &mut self,
        _ante: i32,
        prob_mult: u32,
        exclude: &[Jokers],
        rarity: Rarity,
    ) -> Jokers {
        self.joker_gen
            .gen_joker_of_rarity(prob_mult, exclude, rarity, &mut self.rng)
    }

    fn pick_random_joker(&mut self, available: Vec<Jokers>) -> Jokers {
        let idx = self.rng.gen_range(0..available.len());
        available[idx].clone()
    }

    fn clone_joker(&mut self, j: Jokers) -> Jokers {
        self.joker_gen.clone_joker(j, &mut self.rng)
    }

    fn shuffle_deck(&mut self, deck: &mut Deck) {
        deck.shuffle(&mut self.rng);
    }

    fn prob_roll(&mut self, numerator: u32, denominator: u32) -> bool {
        self.rng.gen_ratio(numerator.min(denominator), denominator)
    }

    fn draw_ante_tags(&mut self) -> (Tag, Tag) {
        let tags: Vec<Tag> = Tag::iter().collect();
        let small = *tags.choose(&mut self.rng).unwrap();
        let big = *tags.choose(&mut self.rng).unwrap();
        (small, big)
    }

    fn draw_boss(&mut self, ante: i32) -> BossBlind {
        let pool: Vec<BossBlind> = BossBlind::iter()
            .filter(|b| b.is_finisher() == (ante % 8 == 0))
            .filter(|b| ante >= b.min_ante())
            .collect();
        *pool.choose(&mut self.rng).unwrap()
    }

    fn roll_random_tarot(&mut self, exclude: &[Tarot]) -> Consumable {
        self.consumable_gen
            .gen_tarot_consumable(exclude, &mut self.rng)
    }

    fn roll_random_planet(&mut self, planetarium: &Planetarium, exclude: &[Planets]) -> Consumable {
        self.consumable_gen
            .gen_planet_consumable(planetarium, exclude, &mut self.rng)
    }

    fn roll_random_spectral(&mut self) -> Spectral {
        random_spectral(&mut self.rng)
    }

    fn roll_int(&mut self, min: u32, max: u32) -> u32 {
        self.rng.gen_range(min..=max)
    }

    fn roll_discard_selector(&mut self, j: &mut Jokers) {
        crate::joker::roll_discard_selector(&mut self.rng, j);
    }

    fn roll_random_edition(&mut self) -> Edition {
        const EDITIONS: [Edition; 3] = [Edition::Foil, Edition::Holographic, Edition::Polychrome];
        EDITIONS[self.rng.gen_range(0..EDITIONS.len())]
    }

    fn roll_random_suit(&mut self) -> Suit {
        let suits: Vec<Suit> = Suit::iter().collect();
        suits[self.rng.gen_range(0..suits.len())]
    }

    fn roll_random_value(&mut self) -> Value {
        let values: Vec<Value> = Value::iter().collect();
        values[self.rng.gen_range(0..values.len())]
    }

    fn pick_random_card(&mut self, available: Vec<Card>) -> Card {
        available[self.rng.gen_range(0..available.len())]
    }

    fn gen_random_card(
        &mut self,
        prob_mult: u32,
        force_enhance: bool,
        force_values: Option<&[Value]>,
    ) -> Card {
        gen_random_playing_card(prob_mult, &mut self.rng, force_enhance, force_values)
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub(crate) struct RealBackend {
    instance: Instance,
    // `Instance` only replays real Balatro's actual seed algorithm, need a
    // separate dedicated fast rng for some other things atm — also used
    // for shop playing cards (Magic Trick): `balatro-seed`'s
    // `ShopItem::PlayingCard` is a marker that carries no card data, and
    // drawing one out of the `Instance` would advance nodes the real game
    // doesn't, desyncing every later draw.
    extra_rng: ChaCha8Rng,
    // Need a fast backend for gen that real doesn't yet impl
    fast: FastBackend,
}

impl RealBackend {
    pub(crate) fn new(seed: &str, fast: FastBackend) -> Self {
        RealBackend {
            instance: Instance::new(seed),
            extra_rng: ChaCha8Rng::seed_from_u64(crate::seed_from_str(seed).wrapping_add(1)),
            fast,
        }
    }

    fn gen_pack_contents(
        &mut self,
        ante: i32,
        category: PackCategory,
        count: i32,
    ) -> Vec<PackContent> {
        match category {
            PackCategory::Arcana => self
                .instance
                .next_arcana_pack(count, ante)
                .into_iter()
                .map(consumable_to_pack_content)
                .collect(),
            PackCategory::Celestial => self
                .instance
                .next_celestial_pack(count, ante)
                .into_iter()
                .map(consumable_to_pack_content)
                .collect(),
            PackCategory::Spectral => self
                .instance
                .next_spectral_pack(count, ante)
                .into_iter()
                .map(consumable_to_pack_content)
                .collect(),
            PackCategory::Buffoon => self
                .instance
                .next_buffoon_pack(count, ante)
                .into_iter()
                .map(|j| seed_joker_with_id(j, &mut self.extra_rng))
                .map(PackContent::Joker)
                .collect(),
            PackCategory::Standard => self
                .instance
                .next_standard_pack(count, ante)
                .into_iter()
                .map(|c| PackContent::PlayingCard(seed_card_to_core_card(c)))
                .collect(),
        }
    }
}

/// `balatro_types::Card` has no `id`; `core::Card` adds one, so only
/// `value`/`suit` go through the id-assigning constructor.
fn seed_card_to_core_card(c: balatro_types::Card) -> Card {
    let mut card = Card::new(c.value, c.suit);
    card.edition = c.edition;
    card.enhancement = c.enhancement;
    card.seal = c.seal;
    card
}

/// Joker generation needs some engine only init at create time
fn seed_joker_with_id(mut j: Jokers, rng: &mut impl Rng) -> Jokers {
    j.set_instance_id(crate::joker::mint_joker_id());
    crate::joker::roll_discard_selector(rng, &mut j);
    j
}

/// `Consumable` may carry Soul/Black Hole (a Spectral) even from a
/// nominally Tarot/Planet draw.
fn consumable_to_pack_content(c: Consumable) -> PackContent {
    match c {
        Consumable::Tarot(t) => PackContent::Tarot(t),
        Consumable::Planet(p) => PackContent::Planet(p),
        Consumable::Spectral(s) => PackContent::Spectral(s),
    }
}

impl RngBackend for RealBackend {
    // `ctx.planetarium` is unused: `Fast` mode uses it to gate secret
    // planets behind discovery state, which isn't wired into `Real` mode
    // TODO. Voucher-driven rates come from the `Instance`'s own active
    // list (kept current by `on_voucher_bought`), not `ctx.vouchers`.
    fn gen_shop_item(
        &mut self,
        ctx: &ShopContext,
        _exclude_jokers: &[Jokers],
        _exclude_tarots: &[Tarot],
        _exclude_planets: &[Planets],
    ) -> GeneratedItem {
        match self.instance.next_shop_item(ctx.ante) {
            balatro_seed::ShopItem::Joker(j) => {
                GeneratedItem::Joker(seed_joker_with_id(j, &mut self.extra_rng))
            }
            balatro_seed::ShopItem::Consumable(c) => GeneratedItem::Consumable(c),
            balatro_seed::ShopItem::PlayingCard => {
                GeneratedItem::PlayingCard(gen_shop_playing_card(
                    ctx.edition_prob_mult(),
                    ctx.vouchers.shop_cards_are_modified(),
                    &mut self.extra_rng,
                ))
            }
        }
    }

    fn gen_pack(
        &mut self,
        ctx: &ShopContext,
        _exclude: Option<(&PackCategory, &PackSize)>,
    ) -> Pack {
        let (category, size) = self.instance.next_pack(ctx.ante);
        let count = balatro_seed::pack_card_count(category, size);
        let contents = self.gen_pack_contents(ctx.ante, category, count);
        Pack {
            category,
            size,
            contents,
        }
    }

    // Unlike `Fast` mode this draws from the full pool and then filters:
    // the draw has to consume the same node the real game would, whether
    // or not the result is still offerable. A base voucher already owned
    // is upgraded in place (`voucher_upgrade`), matching the real game.
    fn gen_voucher(&mut self, ante: i32, vouchers: &Vouchers) -> Option<Voucher> {
        let drawn = self.instance.next_voucher(ante);
        let offer = if vouchers.has(drawn) {
            balatro_seed::voucher_upgrade(drawn).filter(|u| vouchers.is_offerable(*u))
        } else {
            Some(drawn).filter(|v| vouchers.is_offerable(*v))
        };
        if let Some(v) = offer {
            self.instance.lock(&v);
        }
        offer
    }

    fn on_joker_bought(&mut self, joker: &Jokers) {
        self.instance.lock(joker);
    }

    fn on_joker_sold(&mut self, joker: &Jokers) {
        self.instance.unlock(joker);
    }

    fn on_voucher_bought(&mut self, voucher: &Voucher) {
        self.instance.activate_voucher(voucher);
    }

    fn set_showman(&mut self, owned: bool) {
        self.instance.params.showman = owned;
        self.fast.set_showman(owned);
    }

    // `Instance::next_joker(source, ante)` exists and isn't wired up yet,
    // real accuracy here is a follow-up. Reuses the same Fast-style
    // generator `FastBackend` uses, seeded off `extra_rng` instead.
    fn gen_joker(&mut self, ante: i32, prob_mult: u32, exclude: &[Jokers]) -> Jokers {
        self.fast.gen_joker(ante, prob_mult, exclude)
    }

    fn gen_joker_of_rarity(
        &mut self,
        ante: i32,
        prob_mult: u32,
        exclude: &[Jokers],
        rarity: Rarity,
    ) -> Jokers {
        self.fast
            .gen_joker_of_rarity(ante, prob_mult, exclude, rarity)
    }

    fn pick_random_joker(&mut self, available: Vec<Jokers>) -> Jokers {
        self.fast.pick_random_joker(available)
    }

    fn clone_joker(&mut self, j: Jokers) -> Jokers {
        self.fast.clone_joker(j)
    }

    // No real Instance equivalent for deck draw order, yet
    fn shuffle_deck(&mut self, deck: &mut Deck) {
        self.fast.shuffle_deck(deck);
    }

    // No real Instance equivalent for scoring-time probability procs
    // (Lucky/Misprint/SpaceJoker/OopsAllSixes-class effects).
    fn prob_roll(&mut self, numerator: u32, denominator: u32) -> bool {
        self.fast.prob_roll(numerator, denominator)
    }

    // `Instance::next_tag(ante)` exists and isn't wired up yet - real
    // accuracy here is a follow-up.
    fn draw_ante_tags(&mut self) -> (Tag, Tag) {
        self.fast.draw_ante_tags()
    }

    // Correct category (regular vs. finisher) and correct pool-cycling, both
    // handled internally by `next_boss`. Minimum-ante is NOT: real Balatro
    // enforces that by pre-locking each not-yet-eligible boss name via
    // `Instance::init_locks`/`init_unlocks`, which nothing in `core` calls.
    // TODO: Fix min ante for real rng boss gen
    fn draw_boss(&mut self, ante: i32) -> BossBlind {
        self.instance.next_boss(ante)
    }

    // `Instance::next_tarot(source, ante, soulable)` exists and isn't
    // wired up yet - real accuracy here is a follow-up.
    fn roll_random_tarot(&mut self, exclude: &[Tarot]) -> Consumable {
        self.fast.roll_random_tarot(exclude)
    }

    // `Instance::next_planet(source, ante, soulable)` exists and isn't
    // wired up yet - real accuracy here is a follow-up.
    fn roll_random_planet(&mut self, planetarium: &Planetarium, exclude: &[Planets]) -> Consumable {
        self.fast.roll_random_planet(planetarium, exclude)
    }

    fn roll_random_spectral(&mut self) -> Spectral {
        self.fast.roll_random_spectral()
    }

    fn roll_int(&mut self, min: u32, max: u32) -> u32 {
        self.fast.roll_int(min, max)
    }

    fn roll_discard_selector(&mut self, j: &mut Jokers) {
        self.fast.roll_discard_selector(j);
    }

    fn roll_random_edition(&mut self) -> Edition {
        self.fast.roll_random_edition()
    }

    fn roll_random_suit(&mut self) -> Suit {
        self.fast.roll_random_suit()
    }

    fn roll_random_value(&mut self) -> Value {
        self.fast.roll_random_value()
    }

    fn pick_random_card(&mut self, available: Vec<Card>) -> Card {
        self.fast.pick_random_card(available)
    }

    fn gen_random_card(
        &mut self,
        prob_mult: u32,
        force_enhance: bool,
        force_values: Option<&[Value]>,
    ) -> Card {
        self.fast
            .gen_random_card(prob_mult, force_enhance, force_values)
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
// Both variants are inherently chunky — each owns an rng, and `Real` also
// owns the seed `Instance`'s node and lock tables. Exactly one exists per
// `Game`, so boxing would only trade a few hundred bytes for an
// indirection on every draw.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Backend {
    Fast(FastBackend),
    Real(RealBackend),
}

impl RngBackend for Backend {
    fn gen_shop_item(
        &mut self,
        ctx: &ShopContext,
        exclude_jokers: &[Jokers],
        exclude_tarots: &[Tarot],
        exclude_planets: &[Planets],
    ) -> GeneratedItem {
        match self {
            Backend::Fast(b) => {
                b.gen_shop_item(ctx, exclude_jokers, exclude_tarots, exclude_planets)
            }
            Backend::Real(b) => {
                b.gen_shop_item(ctx, exclude_jokers, exclude_tarots, exclude_planets)
            }
        }
    }

    fn gen_pack(&mut self, ctx: &ShopContext, exclude: Option<(&PackCategory, &PackSize)>) -> Pack {
        match self {
            Backend::Fast(b) => b.gen_pack(ctx, exclude),
            Backend::Real(b) => b.gen_pack(ctx, exclude),
        }
    }

    fn gen_voucher(&mut self, ante: i32, vouchers: &Vouchers) -> Option<Voucher> {
        match self {
            Backend::Fast(b) => b.gen_voucher(ante, vouchers),
            Backend::Real(b) => b.gen_voucher(ante, vouchers),
        }
    }

    fn on_joker_bought(&mut self, joker: &Jokers) {
        match self {
            Backend::Fast(b) => b.on_joker_bought(joker),
            Backend::Real(b) => b.on_joker_bought(joker),
        }
    }

    fn on_joker_sold(&mut self, joker: &Jokers) {
        match self {
            Backend::Fast(b) => b.on_joker_sold(joker),
            Backend::Real(b) => b.on_joker_sold(joker),
        }
    }

    fn on_voucher_bought(&mut self, voucher: &Voucher) {
        match self {
            Backend::Fast(b) => b.on_voucher_bought(voucher),
            Backend::Real(b) => b.on_voucher_bought(voucher),
        }
    }

    fn set_showman(&mut self, owned: bool) {
        match self {
            Backend::Fast(b) => b.set_showman(owned),
            Backend::Real(b) => b.set_showman(owned),
        }
    }

    fn gen_joker(&mut self, ante: i32, prob_mult: u32, exclude: &[Jokers]) -> Jokers {
        match self {
            Backend::Fast(b) => b.gen_joker(ante, prob_mult, exclude),
            Backend::Real(b) => b.gen_joker(ante, prob_mult, exclude),
        }
    }

    fn gen_joker_of_rarity(
        &mut self,
        ante: i32,
        prob_mult: u32,
        exclude: &[Jokers],
        rarity: Rarity,
    ) -> Jokers {
        match self {
            Backend::Fast(b) => b.gen_joker_of_rarity(ante, prob_mult, exclude, rarity),
            Backend::Real(b) => b.gen_joker_of_rarity(ante, prob_mult, exclude, rarity),
        }
    }

    fn pick_random_joker(&mut self, available: Vec<Jokers>) -> Jokers {
        match self {
            Backend::Fast(b) => b.pick_random_joker(available),
            Backend::Real(b) => b.pick_random_joker(available),
        }
    }

    fn clone_joker(&mut self, j: Jokers) -> Jokers {
        match self {
            Backend::Fast(b) => b.clone_joker(j),
            Backend::Real(b) => b.clone_joker(j),
        }
    }

    fn shuffle_deck(&mut self, deck: &mut Deck) {
        match self {
            Backend::Fast(b) => b.shuffle_deck(deck),
            Backend::Real(b) => b.shuffle_deck(deck),
        }
    }

    fn prob_roll(&mut self, numerator: u32, denominator: u32) -> bool {
        match self {
            Backend::Fast(b) => b.prob_roll(numerator, denominator),
            Backend::Real(b) => b.prob_roll(numerator, denominator),
        }
    }

    fn draw_ante_tags(&mut self) -> (Tag, Tag) {
        match self {
            Backend::Fast(b) => b.draw_ante_tags(),
            Backend::Real(b) => b.draw_ante_tags(),
        }
    }

    fn draw_boss(&mut self, ante: i32) -> BossBlind {
        match self {
            Backend::Fast(b) => b.draw_boss(ante),
            Backend::Real(b) => b.draw_boss(ante),
        }
    }

    fn roll_random_tarot(&mut self, exclude: &[Tarot]) -> Consumable {
        match self {
            Backend::Fast(b) => b.roll_random_tarot(exclude),
            Backend::Real(b) => b.roll_random_tarot(exclude),
        }
    }

    fn roll_random_planet(&mut self, planetarium: &Planetarium, exclude: &[Planets]) -> Consumable {
        match self {
            Backend::Fast(b) => b.roll_random_planet(planetarium, exclude),
            Backend::Real(b) => b.roll_random_planet(planetarium, exclude),
        }
    }

    fn roll_random_spectral(&mut self) -> Spectral {
        match self {
            Backend::Fast(b) => b.roll_random_spectral(),
            Backend::Real(b) => b.roll_random_spectral(),
        }
    }

    fn roll_int(&mut self, min: u32, max: u32) -> u32 {
        match self {
            Backend::Fast(b) => b.roll_int(min, max),
            Backend::Real(b) => b.roll_int(min, max),
        }
    }

    fn roll_discard_selector(&mut self, j: &mut Jokers) {
        match self {
            Backend::Fast(b) => b.roll_discard_selector(j),
            Backend::Real(b) => b.roll_discard_selector(j),
        }
    }
    fn roll_random_edition(&mut self) -> Edition {
        match self {
            Backend::Fast(b) => b.roll_random_edition(),
            Backend::Real(b) => b.roll_random_edition(),
        }
    }

    fn roll_random_suit(&mut self) -> Suit {
        match self {
            Backend::Fast(b) => b.roll_random_suit(),
            Backend::Real(b) => b.roll_random_suit(),
        }
    }

    fn roll_random_value(&mut self) -> Value {
        match self {
            Backend::Fast(b) => b.roll_random_value(),
            Backend::Real(b) => b.roll_random_value(),
        }
    }

    fn pick_random_card(&mut self, available: Vec<Card>) -> Card {
        match self {
            Backend::Fast(b) => b.pick_random_card(available),
            Backend::Real(b) => b.pick_random_card(available),
        }
    }

    fn gen_random_card(
        &mut self,
        prob_mult: u32,
        force_enhance: bool,
        force_values: Option<&[Value]>,
    ) -> Card {
        match self {
            Backend::Fast(b) => b.gen_random_card(prob_mult, force_enhance, force_values),
            Backend::Real(b) => b.gen_random_card(prob_mult, force_enhance, force_values),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::joker::JokerEffects;

    #[test]
    fn fast_backend_draw_ante_tags_draws_two_valid_tags() {
        let mut backend = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let (small, big) = backend.draw_ante_tags();
        assert!(Tag::iter().any(|t| t == small));
        assert!(Tag::iter().any(|t| t == big));
    }

    #[test]
    fn fast_backend_draw_boss_matches_category() {
        let mut backend = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        for ante in [1, 7, 8, 9, 16] {
            let boss = backend.draw_boss(ante);
            assert_eq!(
                boss.is_finisher(),
                ante % 8 == 0,
                "ante {ante} drew {boss:?}"
            );
        }
    }

    #[test]
    fn fast_backend_draw_boss_respects_min_ante() {
        let mut backend = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        for _ in 0..200 {
            let boss = backend.draw_boss(1);
            assert!(boss.min_ante() <= 1, "ante 1 drew {boss:?} (min_ante > 1)");
        }
    }

    #[test]
    fn fast_backend_prob_roll_respects_ratio_edges() {
        let mut backend = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        // numerator 0 -> never; numerator == denominator -> always,
        // regardless of what's drawn.
        for _ in 0..20 {
            assert!(!backend.prob_roll(0, 10));
        }
        for _ in 0..20 {
            assert!(backend.prob_roll(10, 10));
        }
        // numerator > denominator gets clamped to denominator (always true),
        // not a `gen_ratio` panic.
        assert!(backend.prob_roll(50, 10));
    }

    #[test]
    fn fast_backend_shuffle_deck_reorders_without_losing_cards() {
        let mut backend = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut deck = crate::deck::Deck::default();
        let before = deck.cards();
        backend.shuffle_deck(&mut deck);
        let after = deck.cards();

        assert_eq!(before.len(), after.len());
        assert_ne!(before, after, "shuffle with a fixed seed should reorder");
        let mut sorted_before = before.clone();
        let mut sorted_after = after.clone();
        sorted_before.sort();
        sorted_after.sort();
        assert_eq!(
            sorted_before, sorted_after,
            "shuffle must permute, not lose or duplicate cards"
        );
    }

    // `RealBackend`'s new methods all fall back to `extra_rng` (no real
    // `Instance` equivalent wired up yet, see each method's doc comment) -
    // this is a smoke test that the fallback wiring is complete and doesn't
    // panic, not a claim of real-Balatro accuracy.
    #[test]
    fn real_backend_fallback_methods_all_work() {
        let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut backend = RealBackend::new("TESTSEED", fast);

        let mut deck = crate::deck::Deck::default();
        let before = deck.cards();
        backend.shuffle_deck(&mut deck);
        assert_ne!(before, deck.cards());

        assert!(!backend.prob_roll(0, 10));
        assert!(backend.prob_roll(10, 10));

        let (small, big) = backend.draw_ante_tags();
        assert!(Tag::iter().any(|t| t == small));
        assert!(Tag::iter().any(|t| t == big));

        let planetarium = Planetarium::new();
        let _ = backend.roll_random_planet(&planetarium, &[]);
        let _ = backend.roll_random_tarot(&[]);

        let joker = backend.gen_joker(1, 1, &[]);
        assert!(joker.is_implemented());
    }

    #[test]
    fn real_backend_draw_boss_matches_category() {
        let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut backend = RealBackend::new("TESTSEED", fast);
        for ante in [1, 7, 8, 9, 16] {
            let boss = backend.draw_boss(ante);
            assert_eq!(
                boss.is_finisher(),
                ante % 8 == 0,
                "ante {ante} drew {boss:?}"
            );
        }
    }

    #[test]
    fn real_backend_set_showman_propagates() {
        let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut backend = RealBackend::new("TESTSEED", fast);
        assert!(!backend.instance.params.showman);
        backend.set_showman(true);
        assert!(backend.instance.params.showman);
        backend.set_showman(false);
        assert!(!backend.instance.params.showman);
    }

    // Structural check on gen_pack's typed plumbing across many draws.
    #[test]
    fn real_backend_gen_pack_contents_match_category_and_count() {
        let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut backend = RealBackend::new("TESTSEED", fast);
        let planetarium = Planetarium::new();
        let vouchers = Vouchers::new();
        let mut seen_categories: std::collections::HashSet<PackCategory> =
            std::collections::HashSet::new();

        for ante in 1..=300 {
            let ctx = ShopContext::for_test(&planetarium, &vouchers, ante);
            let pack = backend.gen_pack(&ctx, None);
            seen_categories.insert(pack.category);

            let expected_count = balatro_seed::pack_card_count(pack.category, pack.size);
            assert_eq!(
                pack.contents.len() as i32,
                expected_count,
                "ante {ante}: {:?}/{:?} should hold {expected_count} cards",
                pack.category,
                pack.size
            );

            for content in &pack.contents {
                let matches = match pack.category {
                    PackCategory::Buffoon => matches!(content, PackContent::Joker(_)),
                    PackCategory::Standard => matches!(content, PackContent::PlayingCard(_)),
                    PackCategory::Spectral => matches!(content, PackContent::Spectral(_)),
                    PackCategory::Arcana => {
                        matches!(content, PackContent::Tarot(_) | PackContent::Spectral(_))
                    }
                    PackCategory::Celestial => {
                        matches!(content, PackContent::Planet(_) | PackContent::Spectral(_))
                    }
                };
                assert!(
                    matches,
                    "ante {ante}: {:?} pack produced unexpected content {content:?}",
                    pack.category
                );
            }
        }

        assert_eq!(
            seen_categories.len(),
            5,
            "expected all 5 pack categories across 300 draws, saw {seen_categories:?}"
        );
    }

    // regression: could offer a tier-2 upgrade before its base was ever redeemed.
    #[test]
    fn real_backend_gen_voucher_never_offers_an_ungated_upgrade() {
        let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut backend = RealBackend::new("TESTSEED", fast);
        let vouchers = Vouchers::new();
        let mut saw_any = false;

        for ante in 1..=300 {
            if let Some(v) = backend.gen_voucher(ante, &vouchers) {
                saw_any = true;
                assert!(
                    vouchers.is_offerable(v),
                    "ante {ante}: offered {v:?} with its base not owned"
                );
            }
        }

        assert!(saw_any, "no voucher drawn across 300 antes");
    }

    // `seed_joker_with_id` is `RealBackend`'s equivalent mint chokepoint to
    // `JokerGenerator::gen_joker` (Fast mode) - Castle/MailInRebate must
    // come out with a selector already set via `RealBackend`'s own
    // `extra_rng`, not left `None`. Calls it directly with constructed
    // jokers rather than sampling real-seed shop/pack generation and hoping
    // to land on these two specific jokers among real Balatro's full
    // 150-joker pool, which isn't reliably hit in any bounded sample.
    #[test]
    fn real_backend_seed_joker_with_id_rolls_discard_selector() {
        use crate::joker::{Castle, MailInRebate};

        let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut backend = RealBackend::new("TESTSEED", fast);
        let castle = seed_joker_with_id(Jokers::Castle(Castle::default()), &mut backend.extra_rng);
        assert!(
            castle.state().selector.is_some(),
            "Castle minted with no selector"
        );
        let mail = seed_joker_with_id(
            Jokers::MailInRebate(MailInRebate::default()),
            &mut backend.extra_rng,
        );
        assert!(
            mail.state().selector.is_some(),
            "MailInRebate minted with no selector"
        );
    }
}
