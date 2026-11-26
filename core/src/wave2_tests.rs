//! Wave 2 joker coverage tests.

use crate::card::{Card, Enhancement, Suit, Value};
use crate::effect::GameEvent;
use crate::game::{Game, ProbMode};
use crate::hand::SelectHand;
use crate::joker::*;
use crate::rank::HandRank;
use crate::stage::Blind;

fn add(g: &mut Game, mut joker: Jokers) {
    joker.set_instance_id(g.jokers.len() + 1);
    g.jokers.push(joker);
}

fn register(g: &mut Game) {
    g.register_jokers();
}

fn score(g: &mut Game, cards: Vec<Card>) -> usize {
    let hand = SelectHand::new(cards).best_hand().unwrap();
    g.calc_score(hand)
}

fn card(value: Value, suit: Suit) -> Card {
    Card::new(value, suit)
}

// --- Phase 2 -----------------------------------------------------------

#[test]
fn juggler_adds_hand_size() {
    let mut g = Game::default();
    add(&mut g, Jokers::Juggler(Juggler::default()));
    register(&mut g);
    assert_eq!(g.hand_size(), g.config.available + 1);
}

#[test]
fn drunkard_and_merry_andy_change_discards() {
    let mut g = Game::default();
    add(&mut g, Jokers::Drunkard(Drunkard::default()));
    register(&mut g);
    assert_eq!(g.discards_per_round(), g.config.discards + 1);

    add(&mut g, Jokers::MerryAndy(MerryAndy::default()));
    register(&mut g);
    assert_eq!(g.discards_per_round(), g.config.discards + 4);
    assert_eq!(g.hand_size(), g.config.available.saturating_sub(1));
}

#[test]
fn troubadour_trades_a_hand_for_hand_size() {
    let mut g = Game::default();
    add(&mut g, Jokers::Troubadour(Troubadour::default()));
    register(&mut g);
    assert_eq!(g.hand_size(), g.config.available + 2);
    assert_eq!(g.plays_per_round(), g.config.plays - 1);
}

#[test]
fn stuntman_adds_chips_and_costs_hand_size() {
    let mut g = Game::default();
    add(&mut g, Jokers::Stuntman(Stuntman::default()));
    register(&mut g);
    assert_eq!(g.hand_size(), g.config.available - 2);
    let ace = card(Value::Ace, Suit::Heart);
    assert_eq!(score(&mut g, vec![ace]), 5 + 11 + 250);
}

#[test]
fn turtle_bean_shrinks_at_round_end() {
    let mut g = Game::default();
    add(&mut g, Jokers::TurtleBean(TurtleBean::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().counter = 5.0;
    g.joker_state_mut(1).unwrap().aux = 1.0;
    assert_eq!(g.hand_size(), g.config.available + 5);
    g.emit(GameEvent::RoundEnd);
    assert_eq!(g.joker_state_mut(1).unwrap().counter, 4.0);
}

#[test]
fn oops_doubles_probability_numerators() {
    let mut g = Game::default();
    add(&mut g, Jokers::OopsAllSixes(OopsAllSixes::default()));
    register(&mut g);
    g.prob_mode = ProbMode::Count;
    g.prob_trace.clear();
    let _ = g.prob_roll(1, 4);
    assert_eq!(g.prob_trace, vec![(2, 4)]);
}

#[test]
fn blackboard_needs_all_held_cards_black() {
    let mut g = Game::default();
    add(&mut g, Jokers::Blackboard(Blackboard::default()));
    register(&mut g);
    let ace = card(Value::Ace, Suit::Heart);
    let king = card(Value::King, Suit::Spade);
    // Held cards: one red -> no bonus.
    g.available = crate::available::Available::from_cards(vec![(king, false), (ace, false)]);
    let hand = SelectHand::new(vec![card(Value::Queen, Suit::Club)])
        .best_hand()
        .unwrap();
    assert_eq!(g.calc_score(hand), 15);

    // Only black cards held -> X3.
    let mut g = Game::default();
    add(&mut g, Jokers::Blackboard(Blackboard::default()));
    register(&mut g);
    g.available = crate::available::Available::from_cards(vec![(king, false)]);
    let hand = SelectHand::new(vec![card(Value::Queen, Suit::Club)])
        .best_hand()
        .unwrap();
    assert_eq!(g.calc_score(hand), 15 * 3);
}

#[test]
fn flower_pot_needs_all_four_suits() {
    let mut g = Game::default();
    add(&mut g, Jokers::FlowerPot(FlowerPot::default()));
    register(&mut g);
    let cards = vec![
        card(Value::Ace, Suit::Heart),
        card(Value::King, Suit::Diamond),
        card(Value::Queen, Suit::Club),
        card(Value::Jack, Suit::Spade),
        card(Value::Ten, Suit::Heart),
    ];
    // Straight: (30 + 51) * (4 * 3) = 81 * 12 = 972.
    assert_eq!(score(&mut g, cards), 972);
}

#[test]
fn seeing_double_needs_a_club_and_another_suit() {
    let mut g = Game::default();
    add(&mut g, Jokers::SeeingDouble(SeeingDouble::default()));
    register(&mut g);
    let mixed = vec![card(Value::Ace, Suit::Heart), card(Value::Ace, Suit::Club)];
    // Pair: (10 + 11 + 11) * (2 * 2) = 128.
    assert_eq!(score(&mut g, mixed), 128);
}

#[test]
fn triboulet_doubles_per_king_or_queen() {
    let mut g = Game::default();
    add(&mut g, Jokers::Triboulet(Triboulet::default()));
    register(&mut g);
    let cards = vec![
        card(Value::King, Suit::Heart),
        card(Value::King, Suit::Diamond),
        card(Value::Queen, Suit::Club),
        card(Value::Nine, Suit::Spade),
    ];
    // High card only scores one King: (5 + 10) * (1 * 2) = 30. The made hand
    // is a pair of Kings, actually: (10 + 10 + 10) * (2 * 2 * 2) = 240.
    assert_eq!(score(&mut g, cards), 240);
}

#[test]
fn bootstraps_scales_with_money() {
    let mut g = Game::default();
    add(&mut g, Jokers::Bootstraps(Bootstraps::default()));
    register(&mut g);
    g.money = 23;
    let ace = card(Value::Ace, Suit::Heart);
    // (5 + 11) * (1 + 8) = 144.
    assert_eq!(score(&mut g, vec![ace]), 144);
}

#[test]
fn swashbuckler_sums_other_joker_sell_values() {
    let mut g = Game::default();
    add(&mut g, Jokers::Swashbuckler(Swashbuckler::default()));
    add(&mut g, Jokers::TheJoker(TheJoker::default()));
    register(&mut g);
    let ace = card(Value::Ace, Suit::Heart);
    // The Joker sells for 1 and adds +4 Mult; Swashbuckler adds +1:
    // (5 + 11) * (1 + 4 + 1) = 96.
    assert_eq!(score(&mut g, vec![ace]), 96);
}

#[test]
fn misprint_reports_ev_and_range() {
    let mut g = Game::default();
    add(&mut g, Jokers::Misprint(Misprint::default()));
    register(&mut g);
    let ace = card(Value::Ace, Suit::Heart);
    g.available = crate::available::Available::from_cards(vec![(ace, false)]);
    let evaluation = g.calc_score_pure(&[ace]).expect("legal play");
    // (5 + 11) * (1 + 0..23): EV 200, min 16, max 384.
    assert!((evaluation.score - 200.0).abs() < 1e-9);
    assert_eq!(evaluation.min_score, 16);
    assert_eq!(evaluation.max_score, 384);
    assert_eq!(evaluation.triggers, 1);
}

#[test]
fn throwback_scales_with_skips() {
    let mut g = Game::default();
    add(&mut g, Jokers::Throwback(Throwback::default()));
    register(&mut g);
    g.skips = 4;
    let ace = card(Value::Ace, Suit::Heart);
    // (5 + 11) * 2 = 32.
    assert_eq!(score(&mut g, vec![ace]), 32);
}

// --- Phase 3 -----------------------------------------------------------

#[test]
fn runner_gains_and_applies_same_hand() {
    let mut g = Game::default();
    add(&mut g, Jokers::Runner(Runner::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 15.0;
    let cards = vec![
        card(Value::Ace, Suit::Heart),
        card(Value::King, Suit::Diamond),
        card(Value::Queen, Suit::Club),
        card(Value::Jack, Suit::Spade),
        card(Value::Ten, Suit::Heart),
    ];
    // Straight: (30 + 51 + 15) * 4 = 384.
    assert_eq!(score(&mut g, cards), 384);
    assert_eq!(g.joker_state_mut(1).unwrap().counter, 15.0);
}

#[test]
fn ice_cream_uses_current_then_melts() {
    let mut g = Game::default();
    add(&mut g, Jokers::IceCream(IceCream::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().counter = 100.0;
    g.joker_state_mut(1).unwrap().aux = 5.0;
    let ace = card(Value::Ace, Suit::Heart);
    assert_eq!(score(&mut g, vec![ace]), 5 + 11 + 100);
    assert_eq!(g.joker_state_mut(1).unwrap().counter, 95.0);
}

#[test]
fn square_gains_on_exactly_four_cards() {
    let mut g = Game::default();
    add(&mut g, Jokers::SquareJoker(SquareJoker::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 4.0;
    let cards = vec![
        card(Value::Ace, Suit::Heart),
        card(Value::King, Suit::Diamond),
        card(Value::Queen, Suit::Club),
        card(Value::Jack, Suit::Spade),
    ];
    // High Card Ace: 5 + 11 + 4 = 20; kickers do not score.
    assert_eq!(score(&mut g, cards), 20);
    assert_eq!(g.joker_state_mut(1).unwrap().counter, 4.0);
}

#[test]
fn popcorn_decays_at_round_end() {
    let mut g = Game::default();
    add(&mut g, Jokers::Popcorn(Popcorn::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().counter = 20.0;
    g.joker_state_mut(1).unwrap().aux = 4.0;
    let ace = card(Value::Ace, Suit::Heart);
    assert_eq!(score(&mut g, vec![ace]), (5 + 11) * 21);
    g.emit(GameEvent::RoundEnd);
    assert_eq!(g.joker_state_mut(1).unwrap().counter, 16.0);
}

#[test]
fn flash_gains_per_reroll() {
    let mut g = Game::default();
    add(&mut g, Jokers::FlashCard(FlashCard::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 2.0;
    g.emit(GameEvent::Reroll);
    g.emit(GameEvent::Reroll);
    let ace = card(Value::Ace, Suit::Heart);
    assert_eq!(score(&mut g, vec![ace]), (5 + 11) * 5);
}

#[test]
fn red_card_gains_per_skipped_pack() {
    let mut g = Game::default();
    add(&mut g, Jokers::RedCard(RedCard::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 3.0;
    g.emit(GameEvent::PackSkipped);
    let ace = card(Value::Ace, Suit::Heart);
    assert_eq!(score(&mut g, vec![ace]), (5 + 11) * 4);
}

#[test]
fn fortune_teller_gains_per_tarot() {
    let mut g = Game::default();
    add(&mut g, Jokers::FortuneTeller(FortuneTeller::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 1.0;
    g.emit(GameEvent::ConsumableUsed(
        crate::consumable::Consumable::Tarot(crate::tarot::Tarot::Fool),
    ));
    let ace = card(Value::Ace, Suit::Heart);
    assert_eq!(score(&mut g, vec![ace]), (5 + 11) * 2);
}

#[test]
fn constellation_gains_per_planet() {
    let mut g = Game::default();
    add(&mut g, Jokers::Constellation(Constellation::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().counter = 1.0;
    g.joker_state_mut(1).unwrap().aux = 0.1;
    g.emit(GameEvent::ConsumableUsed(
        crate::consumable::Consumable::Planet(crate::planet::Planets::Pluto),
    ));
    let ace = card(Value::Ace, Suit::Heart);
    // (5 + 11) * 1.1 = 17.6 -> floor 17.
    assert_eq!(score(&mut g, vec![ace]), 17);
}

#[test]
fn hologram_gains_per_card_added() {
    let mut g = Game::default();
    add(&mut g, Jokers::Hologram(Hologram::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().counter = 1.0;
    g.joker_state_mut(1).unwrap().aux = 0.25;
    g.add_card_to_deck(card(Value::Two, Suit::Spade));
    let ace = card(Value::Ace, Suit::Heart);
    // (5 + 11) * 1.25 = 20.
    assert_eq!(score(&mut g, vec![ace]), 20);
}

#[test]
fn vampire_gains_and_strips_enhancement_after_scoring() {
    let mut g = Game::default();
    add(&mut g, Jokers::Vampire(Vampire::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().counter = 1.0;
    g.joker_state_mut(1).unwrap().aux = 0.1;
    let mut ace = card(Value::Ace, Suit::Heart);
    ace.enhancement = Some(Enhancement::Bonus);
    g.available = crate::available::Available::from_cards(vec![(ace, false)]);
    // Enhancement is stripped before scoring: (5 + 11) * 1.1 = 17.6 -> 17.
    assert_eq!(score(&mut g, vec![ace]), 17);
    assert_eq!(g.joker_state_mut(1).unwrap().counter, 1.1);
    assert_eq!(g.available.cards()[0].enhancement, None);
}

#[test]
fn glass_joker_gains_on_glass_destruction() {
    let mut g = Game::default();
    add(&mut g, Jokers::GlassJoker(GlassJoker::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().counter = 1.0;
    g.joker_state_mut(1).unwrap().aux = 0.75;
    let mut glass = card(Value::Two, Suit::Heart);
    glass.enhancement = Some(Enhancement::Glass);
    g.available = crate::available::Available::from_cards(vec![(glass, false)]);
    g.destroy_card(glass.id);
    let ace = card(Value::Ace, Suit::Heart);
    // (5 + 11) * 1.75 = 28.
    assert_eq!(score(&mut g, vec![ace]), 28);
}

#[test]
fn canio_gains_on_face_destruction() {
    let mut g = Game::default();
    add(&mut g, Jokers::Canio(Canio::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().counter = 1.0;
    g.joker_state_mut(1).unwrap().aux = 1.0;
    let king = card(Value::King, Suit::Heart);
    g.available = crate::available::Available::from_cards(vec![(king, false)]);
    g.destroy_card(king.id);
    let ace = card(Value::Ace, Suit::Heart);
    // (5 + 11) * 2 = 32.
    assert_eq!(score(&mut g, vec![ace]), 32);
}

#[test]
fn lucky_cat_gains_on_lucky_trigger() {
    let mut g = Game::default();
    add(&mut g, Jokers::LuckyCat(LuckyCat::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().counter = 1.0;
    g.joker_state_mut(1).unwrap().aux = 0.25;
    g.emit(GameEvent::LuckyTriggered);
    let ace = card(Value::Ace, Suit::Heart);
    // (5 + 11) * 1.25 = 20.
    assert_eq!(score(&mut g, vec![ace]), 20);
}

#[test]
fn ramen_loses_mult_per_discarded_card() {
    let mut g = Game::default();
    add(&mut g, Jokers::Ramen(Ramen::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().counter = 2.0;
    g.joker_state_mut(1).unwrap().aux = 0.01;
    let ace = card(Value::Ace, Suit::Heart);
    g.emit(GameEvent::Discard {
        rank: HandRank::HighCard,
        discarded: vec![card(Value::Two, Suit::Spade); 5],
        first: true,
    });
    assert!((g.joker_state_mut(1).unwrap().counter - 1.95).abs() < 1e-4);
    assert_eq!(score(&mut g, vec![ace]), 31);
}

#[test]
fn campfire_gains_per_sale_and_resets_on_boss() {
    let mut g = Game::default();
    add(&mut g, Jokers::Campfire(Campfire::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().counter = 1.0;
    g.joker_state_mut(1).unwrap().aux = 0.25;
    g.emit(GameEvent::JokerSold(Jokers::TheJoker(TheJoker::default())));
    let ace = card(Value::Ace, Suit::Heart);
    // (5 + 11) * 1.25 = 20.
    assert_eq!(score(&mut g, vec![ace]), 20);
    g.emit(GameEvent::BossDefeated);
    assert_eq!(g.joker_state_mut(1).unwrap().counter, 1.0);
}

#[test]
fn ancient_uses_imported_suit_selector() {
    let mut g = Game::default();
    add(&mut g, Jokers::AncientJoker(AncientJoker::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().selector = Some(SelectorValue::Suit(Suit::Heart));
    g.joker_state_mut(1).unwrap().aux = 1.5;
    let cards = vec![
        card(Value::Ace, Suit::Heart),
        card(Value::King, Suit::Heart),
        card(Value::Queen, Suit::Heart),
        card(Value::Jack, Suit::Heart),
        card(Value::Nine, Suit::Heart),
    ];
    // Flush: (35 + 50) * 4 * 1.5^5 = 340 * 7.59375 = 2581.875 -> 2581.
    assert_eq!(score(&mut g, cards), 2581);
}

#[test]
fn idol_uses_imported_rank_and_suit() {
    let mut g = Game::default();
    add(&mut g, Jokers::TheIdol(TheIdol::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().selector =
        Some(SelectorValue::RankSuit(Value::King, Suit::Heart));
    g.joker_state_mut(1).unwrap().aux = 2.0;
    let cards = vec![
        card(Value::King, Suit::Heart),
        card(Value::King, Suit::Heart),
    ];
    // Pair: (10 + 10 + 10) * (2 * 2^2) = 240.
    assert_eq!(score(&mut g, cards), 240);
}

#[test]
fn todo_list_pays_on_selected_hand() {
    let mut g = Game::default();
    add(&mut g, Jokers::ToDoList(ToDoList::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().counter = 4.0;
    g.joker_state_mut(1).unwrap().selector = Some(SelectorValue::HandRank(HandRank::OnePair));
    let cards = vec![
        card(Value::Ace, Suit::Heart),
        card(Value::Ace, Suit::Diamond),
    ];
    let before = g.money;
    score(&mut g, cards);
    assert_eq!(g.money, before + 4);
}

#[test]
fn loyalty_card_counts_down_and_pays() {
    let mut g = Game::default();
    add(&mut g, Jokers::LoyaltyCard(LoyaltyCard::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().counter = 1.0;
    g.joker_state_mut(1).unwrap().aux = 5.0;
    let ace = card(Value::Ace, Suit::Heart);
    assert_eq!(score(&mut g, vec![ace]), (5 + 11) * 4);
    assert_eq!(g.joker_state_mut(1).unwrap().counter, 5.0);
}

// --- Phase 4 -----------------------------------------------------------

#[test]
fn egg_grows_sell_value_at_round_end() {
    let mut g = Game::default();
    add(&mut g, Jokers::Egg(Egg::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 3.0;
    let before = g.jokers[0].sell_value();
    g.emit(GameEvent::RoundEnd);
    assert_eq!(g.jokers[0].sell_value(), before + 3);
}

#[test]
fn gift_card_grows_every_sell_value() {
    let mut g = Game::default();
    add(&mut g, Jokers::GiftCard(GiftCard::default()));
    add(&mut g, Jokers::TheJoker(TheJoker::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 1.0;
    g.emit(GameEvent::RoundEnd);
    assert_eq!(g.consumable_sell_bonus, 1);
    assert!(g.jokers.iter().all(|joker| joker.state().sell_bonus == 1));
}

#[test]
fn cloud_9_pays_per_nine_in_deck() {
    let mut g = Game::default();
    g.deck = crate::deck::Deck::new();
    add(&mut g, Jokers::Cloud9(Cloud9::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 1.0;
    g.deck.push(card(Value::Nine, Suit::Heart));
    g.deck.push(card(Value::Nine, Suit::Diamond));
    let before = g.money;
    g.emit(GameEvent::RoundEnd);
    assert_eq!(g.money, before + 2);
}

#[test]
fn rocket_pays_and_grows_on_boss() {
    let mut g = Game::default();
    add(&mut g, Jokers::Rocket(Rocket::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().counter = 1.0;
    g.joker_state_mut(1).unwrap().aux = 2.0;
    let before = g.money;
    g.emit(GameEvent::RoundEnd);
    assert_eq!(g.money, before + 1);
    g.emit(GameEvent::BossDefeated);
    assert_eq!(g.joker_state_mut(1).unwrap().counter, 3.0);
}

#[test]
fn to_the_moon_adds_interest() {
    let mut g = Game::default();
    add(&mut g, Jokers::ToTheMoon(ToTheMoon::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 1.0;
    g.money = 23;
    let before = g.money;
    g.emit(GameEvent::RoundEnd);
    assert_eq!(g.money, before + 4);
}

#[test]
fn delayed_gratification_pays_unused_discards() {
    let mut g = Game::default();
    add(
        &mut g,
        Jokers::DelayedGratification(DelayedGratification::default()),
    );
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 2.0;
    g.discards_remaining = 3;
    let before = g.money;
    g.emit(GameEvent::RoundEnd);
    assert_eq!(g.money, before + 6);
}

#[test]
fn satellite_pays_per_unique_planet() {
    let mut g = Game::default();
    add(&mut g, Jokers::Satellite(Satellite::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 1.0;
    g.planets_used.insert(crate::planet::Planets::Pluto);
    g.planets_used.insert(crate::planet::Planets::Mercury);
    let before = g.money;
    g.emit(GameEvent::RoundEnd);
    assert_eq!(g.money, before + 2);
}

#[test]
fn cavendish_is_x3() {
    let mut g = Game::default();
    add(&mut g, Jokers::Cavendish(Cavendish::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 3.0;
    let ace = card(Value::Ace, Suit::Heart);
    assert_eq!(score(&mut g, vec![ace]), (5 + 11) * 3);
}

#[test]
fn mr_bones_saves_a_final_hand_above_25_percent() {
    let mut g = Game::default();
    add(&mut g, Jokers::MrBones(MrBones::default()));
    register(&mut g);
    g.stage = crate::stage::Stage::Blind(Blind::Small);
    g.blind = Some(Blind::Small);
    g.required_score_override = Some(100);
    g.score = 40;
    g.plays = 0;
    let cleared = g.handle_score(0).expect("scoring should not error");
    assert!(cleared);
    assert!(g.jokers.is_empty());
}

#[test]
fn invisible_joker_duplicates_after_two_rounds() {
    let mut g = Game::default();
    add(&mut g, Jokers::InvisibleJoker(InvisibleJoker::default()));
    add(&mut g, Jokers::TheJoker(TheJoker::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().counter = 2.0;
    g.joker_state_mut(1).unwrap().aux = 2.0;
    let before = g.jokers.len();
    g.sell_joker(0).expect("sale");
    assert_eq!(g.jokers.len(), before);
}

// --- Phase 5 -----------------------------------------------------------

#[test]
fn ceremonial_dagger_eats_right_neighbor() {
    let mut g = Game::default();
    add(
        &mut g,
        Jokers::CeremonialDagger(CeremonialDagger::default()),
    );
    add(&mut g, Jokers::TheJoker(TheJoker::default()));
    register(&mut g);
    let sell = g.jokers[1].sell_value();
    g.emit(GameEvent::BlindSelected(Blind::Small));
    assert_eq!(g.jokers.len(), 1);
    assert_eq!(g.joker_state_mut(1).unwrap().counter, (sell * 2) as f32);
}

#[test]
fn marble_adds_stone_card() {
    let mut g = Game::default();
    add(&mut g, Jokers::MarbleJoker(MarbleJoker::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 1.0;
    let before = g.full_deck().len();
    g.emit(GameEvent::BlindSelected(Blind::Small));
    assert_eq!(g.full_deck().len(), before + 1);
    assert!(g
        .deck
        .cards()
        .iter()
        .any(|card| card.enhancement == Some(Enhancement::Stone)));
}

#[test]
fn burglar_adds_hands_and_removes_discards() {
    let mut g = Game::default();
    add(&mut g, Jokers::Burglar(Burglar::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 3.0;
    g.plays = 4;
    g.discards_remaining = 4;
    g.emit(GameEvent::BlindSelected(Blind::Small));
    assert_eq!(g.plays, 7);
    assert_eq!(g.discards_remaining, 0);
}

#[test]
fn riff_raff_creates_two_jokers() {
    let mut g = Game::default();
    add(&mut g, Jokers::RiffRaff(RiffRaff::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 2.0;
    g.emit(GameEvent::BlindSelected(Blind::Small));
    assert_eq!(g.jokers.len(), 3);
}

#[test]
fn cartomancer_creates_tarot() {
    let mut g = Game::default();
    add(&mut g, Jokers::Cartomancer(Cartomancer::default()));
    register(&mut g);
    g.emit(GameEvent::BlindSelected(Blind::Small));
    assert_eq!(g.consumables.len(), 1);
}

#[test]
fn certificate_adds_sealed_card_to_hand() {
    let mut g = Game::default();
    add(&mut g, Jokers::Certificate(Certificate::default()));
    register(&mut g);
    g.emit(GameEvent::BlindSelected(Blind::Small));
    assert_eq!(g.available.cards().len(), 1);
    assert!(g.available.cards()[0].seal.is_some());
}

#[test]
fn dna_copies_a_single_card_on_the_first_hand() {
    let mut g = Game::default();
    add(&mut g, Jokers::Dna(Dna::default()));
    register(&mut g);
    let ace = card(Value::Ace, Suit::Heart);
    g.emit(GameEvent::HandPlayed {
        rank: HandRank::HighCard,
        played: vec![ace],
        scored: vec![ace],
        first: true,
    });
    assert_eq!(g.available.cards().len(), 1);
    assert_eq!(g.draw_adjust, -1);
}

#[test]
fn sixth_sense_destroys_six_and_creates_spectral() {
    let mut g = Game::default();
    add(&mut g, Jokers::SixthSense(SixthSense::default()));
    register(&mut g);
    let six = card(Value::Six, Suit::Heart);
    g.available = crate::available::Available::from_cards(vec![(six, true)]);
    g.emit(GameEvent::HandPlayed {
        rank: HandRank::HighCard,
        played: vec![six],
        scored: vec![six],
        first: true,
    });
    assert!(g.available.cards().is_empty());
    assert_eq!(g.consumables.len(), 1);
    assert_eq!(g.draw_adjust, 0);
}

#[test]
fn eight_ball_creates_tarot_on_scripted_proc() {
    let mut g = Game::default();
    add(&mut g, Jokers::EightBall(EightBall::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 4.0;
    g.prob_mode = ProbMode::Scripted;
    g.prob_script = vec![true];
    let eight = card(Value::Eight, Suit::Heart);
    score(&mut g, vec![eight]);
    assert_eq!(g.consumables.len(), 1);
}

#[test]
fn space_joker_upgrades_hand_level_on_scripted_proc() {
    let mut g = Game::default();
    add(&mut g, Jokers::SpaceJoker(SpaceJoker::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 4.0;
    g.prob_mode = ProbMode::Scripted;
    g.prob_script = vec![true];
    let ace = card(Value::Ace, Suit::Heart);
    let before = g.planetarium.level(HandRank::HighCard).level;
    score(&mut g, vec![ace]);
    assert_eq!(g.planetarium.level(HandRank::HighCard).level, before + 1);
}

#[test]
fn superposition_creates_tarot_for_ace_straight() {
    let mut g = Game::default();
    add(&mut g, Jokers::Superposition(Superposition::default()));
    register(&mut g);
    g.emit(GameEvent::HandPlayed {
        rank: HandRank::Straight,
        played: vec![card(Value::Ace, Suit::Heart)],
        scored: vec![card(Value::Ace, Suit::Heart)],
        first: false,
    });
    assert_eq!(g.consumables.len(), 1);
}

#[test]
fn seance_creates_spectral_for_straight_flush() {
    let mut g = Game::default();
    add(&mut g, Jokers::Seance(Seance::default()));
    register(&mut g);
    g.emit(GameEvent::HandPlayed {
        rank: HandRank::StraightFlush,
        played: Vec::new(),
        scored: Vec::new(),
        first: false,
    });
    assert_eq!(g.consumables.len(), 1);
}

#[test]
fn vagabond_creates_tarot_when_broke() {
    let mut g = Game::default();
    add(&mut g, Jokers::Vagabond(Vagabond::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 4.0;
    g.money = 3;
    g.emit(GameEvent::HandPlayed {
        rank: HandRank::HighCard,
        played: Vec::new(),
        scored: Vec::new(),
        first: false,
    });
    assert_eq!(g.consumables.len(), 1);
}

#[test]
fn burnt_joker_upgrades_first_discard() {
    let mut g = Game::default();
    add(&mut g, Jokers::BurntJoker(BurntJoker::default()));
    register(&mut g);
    g.emit(GameEvent::Discard {
        rank: HandRank::OnePair,
        discarded: vec![card(Value::Two, Suit::Heart)],
        first: true,
    });
    assert_eq!(g.planetarium.level(HandRank::OnePair).level, 2);
}

#[test]
fn hiker_adds_permanent_chips_to_scored_cards() {
    let mut g = Game::default();
    add(&mut g, Jokers::Hiker(Hiker::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 5.0;
    let ace = card(Value::Ace, Suit::Heart);
    g.available = crate::available::Available::from_cards(vec![(ace, false)]);
    g.emit(GameEvent::HandPlayed {
        rank: HandRank::HighCard,
        played: vec![ace],
        scored: vec![ace],
        first: false,
    });
    assert_eq!(g.available.cards()[0].bonus_chips, 5);
}

#[test]
fn madness_gains_and_destroys_a_joker() {
    let mut g = Game::default();
    add(&mut g, Jokers::Madness(Madness::default()));
    add(&mut g, Jokers::TheJoker(TheJoker::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().counter = 1.0;
    g.joker_state_mut(1).unwrap().aux = 0.5;
    g.emit(GameEvent::BlindSelected(Blind::Small));
    assert_eq!(g.joker_state_mut(1).unwrap().counter, 1.5);
    assert_eq!(g.jokers.len(), 1);
}

// --- Phase 6 -----------------------------------------------------------

#[test]
fn credit_card_allows_debt_up_to_twenty() {
    let mut g = Game::default();
    add(&mut g, Jokers::CreditCard(CreditCard::default()));
    register(&mut g);
    g.money = 0;
    g.spend(20).expect("credit limit covers 20");
    assert_eq!(g.debt, 20);
    assert!(g.spend(1).is_err());
}

#[test]
fn chaos_grants_a_free_reroll() {
    let mut g = Game::default();
    add(&mut g, Jokers::ChaosTheClown(ChaosTheClown::default()));
    register(&mut g);
    g.stage = crate::stage::Stage::Shop();
    g.money = 100;
    g.free_rerolls = 1;
    g.reroll().expect("free reroll");
    assert_eq!(g.free_rerolls, 0);
    assert_eq!(g.money, 100);
}

#[test]
fn astronomer_makes_planets_free() {
    let mut g = Game::default();
    add(&mut g, Jokers::Astronomer(Astronomer::default()));
    register(&mut g);
    g.stage = crate::stage::Stage::Shop();
    g.money = 0;
    let planet = crate::consumable::Consumable::Planet(crate::planet::Planets::Pluto);
    g.shop.consumables.push(planet);
    g.buy_consumable(planet).expect("free planet");
    assert_eq!(g.money, 0);
}

#[test]
fn hallucination_creates_tarot_on_pack_open() {
    let mut g = Game::default();
    add(&mut g, Jokers::Hallucination(Hallucination::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().aux = 2.0;
    g.prob_mode = ProbMode::Scripted;
    g.prob_script = vec![true];
    g.emit(GameEvent::PackOpened);
    assert_eq!(g.consumables.len(), 1);
}

#[test]
fn diet_cola_grants_double_tag_on_sell() {
    let mut g = Game::default();
    add(&mut g, Jokers::DietCola(DietCola::default()));
    register(&mut g);
    g.sell_joker(0).expect("sale");
    assert!(g.tags.contains(&crate::tag::Tag::Double));
}

#[test]
fn perkeo_copies_a_consumable_after_shop() {
    let mut g = Game::default();
    add(&mut g, Jokers::Perkeo(Perkeo::default()));
    register(&mut g);
    g.consumables.push(crate::consumable::Consumable::Planet(
        crate::planet::Planets::Pluto,
    ));
    g.emit(GameEvent::ShopEnd);
    assert_eq!(g.negative_consumables.len(), 1);
    assert_eq!(g.consumables.len(), 1);
}

#[test]
fn trading_card_destroys_without_replacement_draw() {
    let mut g = Game::default();
    g.deck = crate::deck::Deck::new();
    add(&mut g, Jokers::TradingCard(TradingCard::default()));
    register(&mut g);
    g.stage = crate::stage::Stage::Blind(Blind::Small);
    g.blind = Some(Blind::Small);
    let a = card(Value::Ace, Suit::Heart);
    let b = card(Value::King, Suit::Spade);
    g.available = crate::available::Available::from_cards(vec![(a, true), (b, false)]);
    g.discards_remaining = 4;
    let money_before = g.money;
    g.discard_selected().expect("discard");
    // The discarded Ace is destroyed, not replaced.
    assert_eq!(g.available.cards().len(), 1);
    assert_eq!(g.money, money_before + 3);
    assert_eq!(g.discards_remaining, 3);
}

#[test]
fn castle_accumulates_chips_per_discarded_suit_card() {
    let mut g = Game::default();
    add(&mut g, Jokers::Castle(Castle::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().selector = Some(SelectorValue::Suit(Suit::Heart));
    g.joker_state_mut(1).unwrap().aux = 3.0;
    g.emit(GameEvent::Discard {
        rank: HandRank::HighCard,
        discarded: vec![
            card(Value::Two, Suit::Heart),
            card(Value::Three, Suit::Heart),
            card(Value::Four, Suit::Spade),
        ],
        first: true,
    });
    assert_eq!(g.joker_state_mut(1).unwrap().counter, 6.0);
    let ace = card(Value::Ace, Suit::Heart);
    // High Card (5 + 11) + 6 Castle chips = 22.
    assert_eq!(score(&mut g, vec![ace]), 22);
}

#[test]
fn photograph_doubles_before_additive_jokers() {
    let mut g = Game::default();
    add(&mut g, Jokers::Popcorn(Popcorn::default()));
    add(&mut g, Jokers::AbstractJoker(AbstractJoker::default()));
    add(&mut g, Jokers::Photograph(Photograph::default()));
    add(&mut g, Jokers::Mime(Mime::default()));
    add(&mut g, Jokers::SteelJoker(SteelJoker::default()));
    register(&mut g);
    g.joker_state_mut(1).unwrap().counter = 8.0;

    let jack = card(Value::Jack, Suit::Heart);
    // High Card J: 5 + 10 = 15 chips.
    // Photograph X2 first (1 -> 2), Popcorn +8 (-> 10), Abstract +15 (-> 25).
    assert_eq!(score(&mut g, vec![jack]), 15 * 25);
}
