use indexmap::IndexMap;
use itertools::Itertools;
#[cfg(feature = "python")]
use pyo3::pyclass;
use std::collections::{BTreeMap, HashSet};
use std::fmt;

use crate::card::Card;
use crate::card::Enhancement;
use crate::card::Suit;
use crate::card::Value;
use crate::error::PlayHandError;
use crate::rank::HandRank;

// Hand, SelectHand and MadeHand are all representations of a collection of Card,
// just at different phases in the cycle of selecting, executing and scoring cards.
// Hand represents all drawn cards, cards available for action (play/discard).
// SelectHand represents (up to 5) cards user selects from hand for action.
// MadeHand represents actual poker hand level and associated cards from a selected hand.

// Hand represents all drawn cards, cards available for action (play/discard)
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Hand(Vec<Card>);

/// Rule flags that change how a selected hand is classified.
#[derive(Debug, Clone, Copy, Default, Hash, PartialEq, Eq)]
pub struct HandRules {
    pub four_fingers: bool,
    pub shortcut: bool,
    pub smeared: bool,
}

// MadeHand represents actual poker hand level and associated cards from a selected hand.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct MadeHand {
    pub hand: SelectHand,
    pub rank: HandRank,
    pub all: Vec<Card>,
}

impl MadeHand {
    /// Cards that score this hand. Normally only the made-hand subset scores,
    /// but Splash makes every played card score. Stone cards are excluded here
    /// because they are handled by the dedicated Stone scoring pass; keeping
    /// them out prevents their edition/seal from being applied twice.
    pub fn scored_cards(&self, splash: bool) -> Vec<Card> {
        if splash {
            self.all
                .iter()
                .filter(|card| card.enhancement != Some(Enhancement::Stone))
                .copied()
                .collect()
        } else {
            self.hand.cards()
        }
    }
}

// SelectHand represents (up to 5) cards user selects from hand for action
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "python", pyclass)]
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct SelectHand(Vec<Card>);

impl SelectHand {
    pub fn new(cards: Vec<Card>) -> Self {
        Self(cards)
    }
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
    // Get all values in a hand. Sorted lowest to highest.
    // Stone cards have no rank and never participate in hand ranking.
    fn values(&self) -> Vec<Value> {
        self.0
            .iter()
            .filter(|card| card.enhancement != Some(Enhancement::Stone))
            .map(|x| x.value)
            .sorted()
            .collect()
    }

    fn rank_value(value: Value) -> u8 {
        match value {
            Value::Two => 2,
            Value::Three => 3,
            Value::Four => 4,
            Value::Five => 5,
            Value::Six => 6,
            Value::Seven => 7,
            Value::Eight => 8,
            Value::Nine => 9,
            Value::Ten => 10,
            Value::Jack => 11,
            Value::Queen => 12,
            Value::King => 13,
            Value::Ace => 14,
        }
    }

    /// Red suits share a group under Smeared Joker; black suits share the other.
    fn suit_group(suit: Suit, smeared: bool) -> u8 {
        if smeared {
            match suit {
                Suit::Heart | Suit::Diamond => 0,
                Suit::Club | Suit::Spade => 1,
            }
        } else {
            suit as u8
        }
    }

    fn order_by_hand(&self, cards: &[Card]) -> SelectHand {
        let ids: HashSet<usize> = cards.iter().map(|card| card.id).collect();
        SelectHand::new(
            self.0
                .iter()
                .filter(|card| ids.contains(&card.id))
                .cloned()
                .collect(),
        )
    }
    pub(crate) fn cards(&self) -> Vec<Card> {
        self.0.clone()
    }

    // Get map of each value with corresponding cards.
    // For example, Ks, Ah, Jh, Jc, Jd -> {A: [Ah], K: [Ks], J: [Jh, Jc: Jd]}
    fn values_freq(&self) -> IndexMap<Value, Vec<Card>> {
        let mut counts: IndexMap<Value, Vec<Card>> = IndexMap::new();
        for card in self.0.clone() {
            if card.enhancement == Some(Enhancement::Stone) {
                continue;
            }
            if let Some(cards) = counts.get(&card.value) {
                let mut copy = cards.clone();
                copy.push(card);
                counts.insert(card.value, copy);
            } else {
                counts.insert(card.value, vec![card]);
            }
        }
        // Return sorted by value
        counts
            .into_iter()
            .sorted_by(|a, b| Ord::cmp(&b.0, &a.0))
            .collect()
    }

    // Original relative order, values_freq()'s groups are order-preserving
    // individually, but concatenating two of them (two pair, full house)
    // isn't, without this.
    fn cards_of_values(&self, values: &[Value]) -> Vec<Card> {
        self.0
            .iter()
            .filter(|c| c.enhancement != Some(Enhancement::Stone) && values.contains(&c.value))
            .cloned()
            .collect()
    }

    /// Can play any number of cards; the caller provides the active rule
    /// flags (Four Fingers / Shortcut / Smeared Joker).
    pub fn best_hand(&self) -> Result<MadeHand, PlayHandError> {
        self.best_hand_with_rules(HandRules::default())
    }

    pub fn best_hand_with_rules(&self, rules: HandRules) -> Result<MadeHand, PlayHandError> {
        if self.0.is_empty() {
            return Err(PlayHandError::NoCards);
        }
        if self.len() > 5 {
            return Err(PlayHandError::TooManyCards);
        }

        // Higher-tier hands take precedence over lower-tier hands regardless
        // of their level or scoring.
        let (hand, rank) = if let Some(hand) = self.is_flush_five_with_rules(rules) {
            (hand, HandRank::FlushFive)
        } else if let Some(hand) = self.is_flush_house_with_rules(rules) {
            (hand, HandRank::FlushHouse)
        } else if let Some(hand) = self.is_five_of_kind() {
            (hand, HandRank::FiveOfAKind)
        } else if let Some(hand) = self.is_royal_flush_with_rules(rules) {
            (hand, HandRank::RoyalFlush)
        } else if let Some(hand) = self.is_straight_flush_with_rules(rules) {
            (hand, HandRank::StraightFlush)
        } else if let Some(hand) = self.is_four_of_kind() {
            (hand, HandRank::FourOfAKind)
        } else if let Some(hand) = self.is_fullhouse() {
            (hand, HandRank::FullHouse)
        } else if let Some(hand) = self.is_flush_with_rules(rules) {
            (hand, HandRank::Flush)
        } else if let Some(hand) = self.is_straight_with_rules(rules) {
            (hand, HandRank::Straight)
        } else if let Some(hand) = self.is_three_of_kind() {
            (hand, HandRank::ThreeOfAKind)
        } else if let Some(hand) = self.is_two_pair() {
            (hand, HandRank::TwoPair)
        } else if let Some(hand) = self.is_pair() {
            (hand, HandRank::OnePair)
        } else if let Some(hand) = self.is_highcard() {
            (hand, HandRank::HighCard)
        } else {
            return Err(PlayHandError::UnknownHand);
        };

        Ok(MadeHand {
            hand,
            rank,
            all: self.cards(),
        })
    }

    pub(crate) fn is_highcard(&self) -> Option<SelectHand> {
        if self.len() < 1 {
            return None;
        }
        let Some((value, _)) = self
            .values_freq()
            .into_iter()
            .find(|(_key, val)| !val.is_empty())
        else {
            // A hand made only of Stone cards is still High Card; the stone
            // kicker scoring loop handles the +50 chips.
            return Some(SelectHand::new(Vec::new()));
        };
        Some(SelectHand::new(self.cards_of_values(&[value])))
    }

    pub(crate) fn is_pair(&self) -> Option<SelectHand> {
        if self.len() < 2 {
            return None;
        }
        let (value, _) = self
            .values_freq()
            .into_iter()
            .find(|(_key, val)| val.len() >= 2)?;
        Some(SelectHand::new(self.cards_of_values(&[value])))
    }

    pub(crate) fn is_two_pair(&self) -> Option<SelectHand> {
        if self.len() < 4 {
            return None;
        }

        let (first_val, _) = self
            .values_freq()
            .into_iter()
            .find(|(_key, val)| val.len() >= 2)?;

        let (second_val, _) = self
            .values_freq()
            .into_iter()
            .find(|(key, val)| *key != first_val && val.len() >= 2)?;

        Some(SelectHand::new(
            self.cards_of_values(&[first_val, second_val]),
        ))
    }

    pub(crate) fn is_three_of_kind(&self) -> Option<SelectHand> {
        if self.len() < 3 {
            return None;
        }
        let (value, _) = self
            .values_freq()
            .into_iter()
            .find(|(_key, val)| val.len() >= 3)?;
        Some(SelectHand::new(self.cards_of_values(&[value])))
    }

    #[cfg(test)]
    pub(crate) fn is_straight(&self) -> Option<SelectHand> {
        self.is_straight_with_rules(HandRules::default())
    }

    pub(crate) fn is_straight_with_rules(&self, rules: HandRules) -> Option<SelectHand> {
        let lengths: &[usize] = if rules.four_fingers { &[5, 4] } else { &[5] };
        for &length in lengths {
            if let Some(hand) = self.straight_of_length(length, rules.shortcut) {
                return Some(hand);
            }
        }
        None
    }

    /// Find a straight of `length` distinct ranks. `shortcut` allows a one-rank
    /// gap between consecutive cards. Ace can be high (14) or low (1).
    fn straight_of_length(&self, length: usize, shortcut: bool) -> Option<SelectHand> {
        let mut by_rank: BTreeMap<u8, Card> = BTreeMap::new();
        let mut ace: Option<Card> = None;
        for card in &self.0 {
            if card.enhancement == Some(Enhancement::Stone) {
                continue;
            }
            let rank = Self::rank_value(card.value);
            by_rank.entry(rank).or_insert(*card);
            if card.value == Value::Ace {
                ace = Some(*card);
            }
        }
        if let Some(ace) = ace {
            by_rank.insert(1, ace);
        }
        if by_rank.len() < length {
            return None;
        }

        let ranks: Vec<u8> = by_rank.keys().copied().collect();
        let mut best: Option<Vec<Card>> = None;
        let mut best_chips = 0;

        for values in ranks.iter().copied().combinations(length) {
            if values.contains(&1) && values.contains(&14) {
                continue;
            }
            let gaps_ok = values.windows(2).all(|window| {
                let gap = window[1] - window[0];
                if shortcut {
                    gap == 1 || gap == 2
                } else {
                    gap == 1
                }
            });
            if !gaps_ok {
                continue;
            }
            let cards: Vec<Card> = values
                .iter()
                .filter_map(|rank| by_rank.get(rank).copied())
                .collect();
            if cards.len() != length {
                continue;
            }
            let chips: usize = cards.iter().map(Card::chips).sum();
            if chips > best_chips {
                best_chips = chips;
                best = Some(cards);
            }
        }

        best.map(|cards| self.order_by_hand(&cards))
    }

    #[cfg(test)]
    pub(crate) fn is_flush(&self) -> Option<SelectHand> {
        self.is_flush_with_rules(HandRules::default())
    }

    pub(crate) fn is_flush_with_rules(&self, rules: HandRules) -> Option<SelectHand> {
        let base_needed = if rules.four_fingers { 4 } else { 5 };
        let wilds: Vec<Card> = self
            .0
            .iter()
            .filter(|card| card.enhancement == Some(Enhancement::Wild))
            .cloned()
            .collect();

        if wilds.len() >= base_needed {
            let non_stone: Vec<Card> = self
                .0
                .iter()
                .filter(|card| card.enhancement != Some(Enhancement::Stone))
                .cloned()
                .collect();
            return Some(SelectHand::new(non_stone));
        }

        let needed = base_needed - wilds.len();
        let mut groups: IndexMap<u8, Vec<Card>> = IndexMap::new();
        for card in self.0.iter().filter(|card| {
            card.enhancement != Some(Enhancement::Wild)
                && card.enhancement != Some(Enhancement::Stone)
        }) {
            let group = Self::suit_group(card.suit, rules.smeared);
            groups.entry(group).or_default().push(*card);
        }

        let mut best: Option<SelectHand> = None;
        let mut best_chips = 0;
        for group in groups.values() {
            if group.len() < needed {
                continue;
            }
            let mut cards = group.clone();
            cards.extend(wilds.iter().copied());
            let ordered = self.order_by_hand(&cards);
            let chips: usize = ordered.cards().iter().map(Card::chips).sum();
            if chips > best_chips {
                best_chips = chips;
                best = Some(ordered);
            }
        }
        best
    }

    pub(crate) fn is_fullhouse(&self) -> Option<SelectHand> {
        if self.len() < 5 {
            return None;
        }

        let (three_val, _) = self
            .values_freq()
            .into_iter()
            .find(|(_key, val)| val.len() >= 3)?;

        let (two_val, _) = self
            .values_freq()
            .into_iter()
            .find(|(key, val)| *key != three_val && val.len() >= 2)?;

        Some(SelectHand::new(self.cards_of_values(&[three_val, two_val])))
    }

    pub(crate) fn is_four_of_kind(&self) -> Option<SelectHand> {
        if self.len() < 4 {
            return None;
        }
        let (value, _) = self
            .values_freq()
            .into_iter()
            .find(|(_key, val)| val.len() >= 4)?;
        Some(SelectHand::new(self.cards_of_values(&[value])))
    }

    #[cfg(test)]
    pub(crate) fn is_straight_flush(&self) -> Option<SelectHand> {
        self.is_straight_flush_with_rules(HandRules::default())
    }

    pub(crate) fn is_straight_flush_with_rules(&self, rules: HandRules) -> Option<SelectHand> {
        let flush = self.is_flush_with_rules(rules)?;
        flush.is_straight_with_rules(rules)
    }

    #[cfg(test)]
    pub(crate) fn is_royal_flush(&self) -> Option<SelectHand> {
        self.is_royal_flush_with_rules(HandRules::default())
    }

    pub(crate) fn is_royal_flush_with_rules(&self, rules: HandRules) -> Option<SelectHand> {
        let hand = self.is_straight_flush_with_rules(rules)?;
        let values = hand.values();
        if values
            == vec![
                Value::Ten,
                Value::Jack,
                Value::Queen,
                Value::King,
                Value::Ace,
            ]
        {
            Some(hand)
        } else {
            None
        }
    }

    pub(crate) fn is_five_of_kind(&self) -> Option<SelectHand> {
        if self.len() < 5 {
            return None;
        }
        let (value, _) = self
            .values_freq()
            .into_iter()
            .find(|(_key, val)| val.len() >= 5)?;
        Some(SelectHand::new(self.cards_of_values(&[value])))
    }

    #[cfg(test)]
    pub(crate) fn is_flush_house(&self) -> Option<SelectHand> {
        self.is_flush_house_with_rules(HandRules::default())
    }

    pub(crate) fn is_flush_house_with_rules(&self, rules: HandRules) -> Option<SelectHand> {
        let flush = self.is_flush_with_rules(rules)?;
        if flush.len() != 5 {
            return None;
        }
        flush.is_fullhouse()
    }

    #[cfg(test)]
    pub(crate) fn is_flush_five(&self) -> Option<SelectHand> {
        self.is_flush_five_with_rules(HandRules::default())
    }

    pub(crate) fn is_flush_five_with_rules(&self, rules: HandRules) -> Option<SelectHand> {
        let flush = self.is_flush_with_rules(rules)?;
        if flush.len() != 5 {
            return None;
        }
        flush.is_five_of_kind()
    }
}

impl Default for SelectHand {
    fn default() -> Self {
        let cards: Vec<Card> = Vec::new();
        Self(cards)
    }
}

impl fmt::Display for SelectHand {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "[")?;
        for card in &self.0 {
            write!(f, "{}", card)?;
        }
        write!(f, "]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_values() {
        let c3 = Card::new(Value::Two, Suit::Heart);
        let c4 = Card::new(Value::Three, Suit::Diamond);
        let c5 = Card::new(Value::Jack, Suit::Heart);
        let c1 = Card::new(Value::King, Suit::Heart);
        let c2 = Card::new(Value::Ace, Suit::Spade);

        let hand = SelectHand::new(vec![c1, c2, c3, c4, c5]);
        let values = hand.values();

        // Should have 5 values
        assert_eq!(values.len(), 5);

        // Expect sorted (2, 3, J, K, A)
        assert_eq!(values[0], Value::Two);
        assert_eq!(values[1], Value::Three);
        assert_eq!(values[2], Value::Jack);
        assert_eq!(values[3], Value::King);
        assert_eq!(values[4], Value::Ace);
    }

    #[test]
    fn test_values_freq() {
        let c1 = Card::new(Value::Two, Suit::Heart);
        let c2 = Card::new(Value::Three, Suit::Diamond);
        let c3 = Card::new(Value::Four, Suit::Heart);
        let c4 = Card::new(Value::King, Suit::Heart);
        let c5 = Card::new(Value::King, Suit::Spade);

        let hand = SelectHand::new(vec![c1, c2, c3, c4, c5]);
        let freq = hand.values_freq();

        // Should have 4 values (K, 2, 3, 4)
        assert_eq!(freq.len(), 4);

        // Expect 2 kings and 1 each of 2, 3, 4
        assert_eq!(freq.get(&Value::King).unwrap().len(), 2);
        assert_eq!(freq.get(&Value::Two).unwrap().len(), 1);
        assert_eq!(freq.get(&Value::Three).unwrap().len(), 1);
        assert_eq!(freq.get(&Value::Four).unwrap().len(), 1);

        // No extra cards
        assert_eq!(freq.get(&Value::Five), None);
        assert_eq!(freq.get(&Value::Nine), None);

        // Can also check the cards in the vec are as expected
        assert_eq!(freq.get(&Value::King).unwrap()[0].value, Value::King);
        assert_eq!(freq.get(&Value::King).unwrap()[1].value, Value::King);
        assert_eq!(freq.get(&Value::Two).unwrap()[0].value, Value::Two);
        assert_eq!(freq.get(&Value::Three).unwrap()[0].value, Value::Three);
        assert_eq!(freq.get(&Value::Four).unwrap()[0].value, Value::Four);

        // Check ordered by value
        assert_eq!(freq.into_iter().next().unwrap().0, Value::King)
    }

    #[test]
    fn test_best_hand() {
        let c1 = Card::new(Value::Ace, Suit::Heart);
        let c2 = Card::new(Value::Two, Suit::Heart);
        let c3 = Card::new(Value::Three, Suit::Diamond);

        // Best hand is flush five (Ah, Ah, Ah, Ah, Ah)
        let hand = SelectHand::new(vec![c1, c1, c1, c1, c1]);
        let best = hand.best_hand().expect("is best hand");
        assert_eq!(best.rank, HandRank::FlushFive);
        assert_eq!(best.hand.len(), 5);

        // 4ok is better than flush (Ah, Ah, Ah, Ah, 2h)
        let hand = SelectHand::new(vec![c1, c1, c1, c1, c2]);
        let best = hand.best_hand().expect("is best hand");
        assert_eq!(best.clone().rank, HandRank::FourOfAKind);
        assert_eq!(best.hand.len(), 4);

        // Two pair is better than pair (Ah, Ah, 2h, 2h, 3d)
        let hand = SelectHand::new(vec![c1, c1, c2, c2, c3]);
        let best = hand.best_hand().expect("is best hand");
        assert_eq!(best.clone().rank, HandRank::TwoPair);
        assert_eq!(best.hand.len(), 4);

        // At worst, we get a high card (Ah, 2h, 3d)
        let hand = SelectHand::new(vec![c1, c2, c3]);
        let best = hand.best_hand().expect("is best hand");
        assert_eq!(best.clone().rank, HandRank::HighCard);
        assert_eq!(best.hand.len(), 1);
    }

    #[test]
    fn test_highcard() {
        let c1 = Card::new(Value::Ace, Suit::Heart);
        let c2 = Card::new(Value::King, Suit::Heart);
        let c3 = Card::new(Value::Three, Suit::Diamond);
        let c4 = Card::new(Value::Four, Suit::Diamond);
        let c5 = Card::new(Value::Five, Suit::Diamond);
        let c6 = Card::new(Value::Six, Suit::Diamond);

        // Valid 5 (A, K, 3, 4, 5)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, c5]);
        let hc = hand.is_highcard();
        assert_eq!(hc.clone().unwrap().len(), 1);
        assert_eq!(hc.unwrap().0[0].value, Value::Ace);

        // Valid 5 (K, A, 3, 4, 5)
        let hand = SelectHand::new(vec![c2, c1, c3, c4, c5]);
        let hc = hand.is_highcard();
        assert_eq!(hc.clone().unwrap().len(), 1);
        assert_eq!(hc.unwrap().0[0].value, Value::Ace);

        // Valid 5 (K, 3, 4, 5, 6)
        let hand = SelectHand::new(vec![c2, c3, c4, c5, c6]);
        let hc = hand.is_highcard();
        assert_eq!(hc.clone().unwrap().len(), 1);
        assert_eq!(hc.unwrap().0[0].value, Value::King);

        // Valid 4 (K, 3, 4, 5)
        let hand = SelectHand::new(vec![c2, c3, c4, c5]);
        let hc = hand.is_highcard();
        assert_eq!(hc.clone().unwrap().len(), 1);
        assert_eq!(hc.unwrap().0[0].value, Value::King);

        // Valid 3 (K, 3, 4)
        let hand = SelectHand::new(vec![c2, c3, c4]);
        let hc = hand.is_highcard();
        assert_eq!(hc.clone().unwrap().len(), 1);
        assert_eq!(hc.unwrap().0[0].value, Value::King);

        // Valid 2 (K, 3)
        let hand = SelectHand::new(vec![c2, c3]);
        let hc = hand.is_highcard();
        assert_eq!(hc.clone().unwrap().len(), 1);
        assert_eq!(hc.unwrap().0[0].value, Value::King);

        // Valid 1 (K)
        let hand = SelectHand::new(vec![c2]);
        let hc = hand.is_highcard();
        assert_eq!(hc.clone().unwrap().len(), 1);
        assert_eq!(hc.unwrap().0[0].value, Value::King);
    }

    #[test]
    fn test_highcard_preserves_order() {
        let ah = Card::new(Value::Ace, Suit::Heart);
        let as_ = Card::new(Value::Ace, Suit::Spade);
        let two = Card::new(Value::Two, Suit::Diamond);

        // Two aces tie for high card — order should follow input order.
        let hand = SelectHand::new(vec![ah, two, as_]);
        let hc = hand.is_highcard().unwrap();
        assert_eq!(hc.cards(), vec![ah, as_]);
    }

    #[test]
    fn test_pair() {
        let c1 = Card::new(Value::King, Suit::Heart);
        let c2 = Card::new(Value::King, Suit::Diamond);
        let c3 = Card::new(Value::Three, Suit::Diamond);
        let c4 = Card::new(Value::Four, Suit::Diamond);
        let c5 = Card::new(Value::Five, Suit::Diamond);
        let c6 = Card::new(Value::Six, Suit::Diamond);

        // Valid 5 (K, K, 3, 4, 5)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, c5]);
        let is_2 = hand.is_pair();
        assert_eq!(is_2.unwrap().len(), 2);

        // Valid 4 (K, K, 3, 4)
        let hand = SelectHand::new(vec![c1, c2, c3, c4]);
        let is_2 = hand.is_pair();
        assert_eq!(is_2.unwrap().len(), 2);

        // Valid 3 (K, K, 3)
        let hand = SelectHand::new(vec![c1, c2, c3]);
        let is_2 = hand.is_pair();
        assert_eq!(is_2.unwrap().len(), 2);

        // Valid 2 (K, K)
        let hand = SelectHand::new(vec![c1, c2]);
        let is_2 = hand.is_pair();
        assert_eq!(is_2.unwrap().len(), 2);

        // Invalid 1 (K)
        let hand = SelectHand::new(vec![c1]);
        let is_2 = hand.is_pair();
        assert_eq!(is_2, None);

        // Invalid 2 (K, 3)
        let hand = SelectHand::new(vec![c1, c3]);
        let is_2 = hand.is_pair();
        assert_eq!(is_2, None);

        // Invalid 3 (K, 3, 4)
        let hand = SelectHand::new(vec![c1, c3, c4]);
        let is_2 = hand.is_pair();
        assert_eq!(is_2, None);

        // Invalid 4 (K, 3, 4, 5)
        let hand = SelectHand::new(vec![c1, c3, c4, c5]);
        let is_2 = hand.is_pair();
        assert_eq!(is_2, None);

        // Invalid 5 (K, 3, 4, 5, 6)
        let hand = SelectHand::new(vec![c1, c3, c4, c5, c6]);
        let is_2 = hand.is_pair();
        assert_eq!(is_2, None);
    }

    #[test]
    fn test_pair_preserves_order() {
        let kh = Card::new(Value::King, Suit::Heart);
        let ks = Card::new(Value::King, Suit::Spade);
        let ah = Card::new(Value::Ace, Suit::Heart);
        let two = Card::new(Value::Two, Suit::Diamond);

        // Kings arranged after the ace kicker, result follows input order.
        let hand = SelectHand::new(vec![ah, kh, two, ks]);
        let pair = hand.is_pair().unwrap();
        assert_eq!(pair.cards(), vec![kh, ks]);
    }

    #[test]
    fn test_two_pair() {
        let c1 = Card::new(Value::King, Suit::Heart);
        let c2 = Card::new(Value::King, Suit::Spade);
        let c3 = Card::new(Value::Four, Suit::Diamond);
        let c4 = Card::new(Value::Four, Suit::Heart);
        let not1 = Card::new(Value::Two, Suit::Heart);
        let not2 = Card::new(Value::Three, Suit::Heart);

        // Valid 5 (K, K, 4, 4, 2)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, not1]);
        let tp = hand.is_two_pair();
        assert_eq!(tp.unwrap().len(), 4);

        // Valid 4 (K, K, 4, 4)
        let hand = SelectHand::new(vec![c1, c2, c3, c4]);
        let tp = hand.is_two_pair();
        assert_eq!(tp.unwrap().len(), 4);

        // Invalid 5 (K, K, K, K, 2)
        let hand = SelectHand::new(vec![c1, c1, c2, c2, not1]);
        let tp = hand.is_two_pair();
        assert_eq!(tp, None);

        // Invalid 5 (K, 4, 3, 2, 2)
        let hand = SelectHand::new(vec![c1, c4, not1, not2, not2]);
        let tp = hand.is_two_pair();
        assert_eq!(tp, None);

        // Invalid 5 (K, K, 4, 3, 2)
        let hand = SelectHand::new(vec![c1, c1, c4, not1, not2]);
        let tp = hand.is_two_pair();
        assert_eq!(tp, None);

        // Invalid 4 (K, K, 4, 2)
        let hand = SelectHand::new(vec![c1, c2, c4, not1]);
        let tp = hand.is_two_pair();
        assert_eq!(tp, None);
    }

    #[test]
    fn test_two_pair_preserves_order() {
        let two_h = Card::new(Value::Two, Suit::Heart);
        let two_s = Card::new(Value::Two, Suit::Spade);
        let kh = Card::new(Value::King, Suit::Heart);
        let ks = Card::new(Value::King, Suit::Spade);

        // Old code always returned the higher-valued pair first regardless
        // of arrangement, this pins the fix.
        let hand = SelectHand::new(vec![two_h, two_s, kh, ks]);
        let tp = hand.is_two_pair().unwrap();
        assert_eq!(tp.cards(), vec![two_h, two_s, kh, ks]);

        // Interleaved arrangement should come out interleaved.
        let hand = SelectHand::new(vec![two_h, kh, two_s, ks]);
        let tp = hand.is_two_pair().unwrap();
        assert_eq!(tp.cards(), vec![two_h, kh, two_s, ks]);
    }

    #[test]
    fn test_three_of_kind() {
        let c1 = Card::new(Value::King, Suit::Heart);
        let c2 = Card::new(Value::King, Suit::Spade);
        let c3 = Card::new(Value::King, Suit::Heart);
        let not1 = Card::new(Value::Ace, Suit::Heart);
        let not2 = Card::new(Value::Two, Suit::Heart);

        // Valid 5 (K, K, K, A, 2)
        let hand = SelectHand::new(vec![c1, c2, c3, not1, not2]);
        let is_3 = hand.is_three_of_kind();
        assert_eq!(is_3.unwrap().len(), 3);

        // Valid 4 (K, K, K, A)
        let hand = SelectHand::new(vec![c1, c2, c3, not1]);
        let is_3 = hand.is_three_of_kind();
        assert_eq!(is_3.unwrap().len(), 3);

        // Valid 3 (K, K, K)
        let hand = SelectHand::new(vec![c1, c2, c3]);
        let is_3 = hand.is_three_of_kind();
        assert_eq!(is_3.unwrap().len(), 3);

        // Invalid 3 (K, K, A)
        let hand = SelectHand::new(vec![c1, c2, not1]);
        let is_3 = hand.is_three_of_kind();
        assert_eq!(is_3, None);

        // Invalid 4 (K, K, A, A),
        let hand = SelectHand::new(vec![c1, c2, not1, not1]);
        let is_3 = hand.is_three_of_kind();
        assert_eq!(is_3, None);

        // Invalid 5 (K, K, A, A, 2),
        let hand = SelectHand::new(vec![c1, c2, not1, not1, not2]);
        let is_3 = hand.is_three_of_kind();
        assert_eq!(is_3, None);

        // Invalid 2 (K, K)
        let hand = SelectHand::new(vec![c1, c2]);
        let is_3 = hand.is_three_of_kind();
        assert_eq!(is_3, None);
    }

    #[test]
    fn test_three_of_kind_preserves_order() {
        let kh = Card::new(Value::King, Suit::Heart);
        let ks = Card::new(Value::King, Suit::Spade);
        let kd = Card::new(Value::King, Suit::Diamond);
        let ah = Card::new(Value::Ace, Suit::Heart);

        // Kings not adjacent in the input.
        let hand = SelectHand::new(vec![kh, ah, ks, kd]);
        let is_3 = hand.is_three_of_kind().unwrap();
        assert_eq!(is_3.cards(), vec![kh, ks, kd]);
    }

    #[test]
    fn test_straight() {
        let c1 = Card::new(Value::Ace, Suit::Heart);
        let c2 = Card::new(Value::Two, Suit::Heart);
        let c3 = Card::new(Value::Three, Suit::Heart);
        let c4 = Card::new(Value::Four, Suit::Heart);
        let c5 = Card::new(Value::Five, Suit::Heart);
        let c6 = Card::new(Value::Six, Suit::Diamond);
        let c7 = Card::new(Value::Seven, Suit::Diamond);

        // Valid 5 (2, 3, 4 ,5 ,6)
        let hand = SelectHand::new(vec![c2, c3, c4, c5, c6]);
        let straight = hand.is_straight();
        assert_eq!(straight.unwrap().len(), 5);

        // Valid 5 with low ace (A, 2, 3, 4 ,5)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, c5]);
        let straight = hand.is_straight();
        assert_eq!(straight.unwrap().len(), 5);

        // Invalid 5 (2, 3, 4, 5, 7)
        let hand = SelectHand::new(vec![c2, c3, c4, c5, c7]);
        let straight = hand.is_straight();
        assert_eq!(straight, None);

        // Invalid 5 with low ace (A, 2, 3, 4, 7)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, c7]);
        let straight = hand.is_straight();
        assert_eq!(straight, None);

        // Invalid 4 (2, 3, 4, 5)
        let hand = SelectHand::new(vec![c2, c3, c4, c5]);
        let straight = hand.is_straight();
        assert_eq!(straight, None);
    }

    #[test]
    fn test_flush() {
        let c1 = Card::new(Value::King, Suit::Heart);
        let c2 = Card::new(Value::Queen, Suit::Heart);
        let c3 = Card::new(Value::Jack, Suit::Heart);
        let c4 = Card::new(Value::Seven, Suit::Heart);
        let c5 = Card::new(Value::Eight, Suit::Heart);
        let not = Card::new(Value::Ace, Suit::Diamond);

        // Valid 5 (h, h, h, h, h)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, c5]);
        let flush = hand.is_flush();
        assert_eq!(flush.unwrap().len(), 5);

        // Valid 5 from 7 cards (h, h, h, h, h, d, d)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, c5, not, not]);
        let flush = hand.is_flush();
        assert_eq!(flush.unwrap().len(), 5);

        // Invalid 5 (h, h, h, h, d)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, not]);
        let flush = hand.is_flush();
        assert_eq!(flush, None);

        // Invalid 4 (h, h, h, h)
        let hand = SelectHand::new(vec![c1, c2, c3, c4]);
        let flush = hand.is_flush();
        assert_eq!(flush, None);
    }

    #[test]
    fn test_fullhouse() {
        let c1 = Card::new(Value::King, Suit::Heart);
        let c2 = Card::new(Value::King, Suit::Spade);
        let c3 = Card::new(Value::King, Suit::Heart);
        let c4 = Card::new(Value::Four, Suit::Diamond);
        let c5 = Card::new(Value::Four, Suit::Heart);
        let not1 = Card::new(Value::Two, Suit::Heart);
        let not2 = Card::new(Value::Three, Suit::Heart);

        // Valid 5 (K, K, K, 4, 4)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, c5]);
        let is_fh = hand.is_fullhouse();
        assert_eq!(is_fh.unwrap().len(), 5);

        // Valid 5 from 7 cards (K, K, K, 4, 4, 2, 3)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, c5, not1, not2]);
        let is_fh = hand.is_fullhouse();
        assert_eq!(is_fh.unwrap().len(), 5);

        // Invalid 5 (K, K, K, K, 2)
        let hand = SelectHand::new(vec![c1, c2, c3, c3, not1]);
        let is_fh = hand.is_fullhouse();
        assert_eq!(is_fh, None);

        // Invalid 5 (K, K, 4, 4, 2)
        let hand = SelectHand::new(vec![c1, c2, c4, c5, not1]);
        let is_fh = hand.is_fullhouse();
        assert_eq!(is_fh, None);

        // Invalid 4 (K, K, 4, 4)
        let hand = SelectHand::new(vec![c1, c2, c4, c5]);
        let is_fh = hand.is_fullhouse();
        assert_eq!(is_fh, None);
    }

    #[test]
    fn test_fullhouse_preserves_order() {
        let two_h = Card::new(Value::Two, Suit::Heart);
        let two_s = Card::new(Value::Two, Suit::Spade);
        let kh = Card::new(Value::King, Suit::Heart);
        let ks = Card::new(Value::King, Suit::Spade);
        let kd = Card::new(Value::King, Suit::Diamond);

        // Pair arranged before the trips, old code always returned the
        // three-of-a-kind group first regardless of arrangement.
        let hand = SelectHand::new(vec![two_h, two_s, kh, ks, kd]);
        let is_fh = hand.is_fullhouse().unwrap();
        assert_eq!(is_fh.cards(), vec![two_h, two_s, kh, ks, kd]);
    }

    #[test]
    fn test_four_of_kind() {
        let c1 = Card::new(Value::King, Suit::Heart);
        let c2 = Card::new(Value::King, Suit::Spade);
        let c3 = Card::new(Value::King, Suit::Heart);
        let c4 = Card::new(Value::King, Suit::Diamond);
        let not = Card::new(Value::Ace, Suit::Heart);

        // Valid 4 (K, K, K, K)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, not]);
        let is_4 = hand.is_four_of_kind();
        assert_eq!(is_4.unwrap().len(), 4);

        // Valid 4 from 7 cards (K, K, K, K, A, A, A)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, not, not, not]);
        let is_4 = hand.is_four_of_kind();
        assert_eq!(is_4.unwrap().len(), 4);

        // Invalid 4 (K, K, K, A)
        let hand = SelectHand::new(vec![c1, c2, c3, not]);
        let is_4 = hand.is_four_of_kind();
        assert_eq!(is_4, None);

        // Invalid 3 (K, K, K)
        let hand = SelectHand::new(vec![c1, c2, c3]);
        let is_4 = hand.is_four_of_kind();
        assert_eq!(is_4, None);
    }

    #[test]
    fn test_four_of_kind_preserves_order() {
        let kh = Card::new(Value::King, Suit::Heart);
        let ks = Card::new(Value::King, Suit::Spade);
        let kd = Card::new(Value::King, Suit::Diamond);
        let kc = Card::new(Value::King, Suit::Club);
        let ah = Card::new(Value::Ace, Suit::Heart);

        let hand = SelectHand::new(vec![kh, ah, ks, kd, kc]);
        let is_4 = hand.is_four_of_kind().unwrap();
        assert_eq!(is_4.cards(), vec![kh, ks, kd, kc]);
    }

    #[test]
    fn test_straight_flush() {
        let c1 = Card::new(Value::Ace, Suit::Heart);
        let c2 = Card::new(Value::Two, Suit::Heart);
        let c3 = Card::new(Value::Three, Suit::Heart);
        let c4 = Card::new(Value::Four, Suit::Heart);
        let c5 = Card::new(Value::Five, Suit::Heart);
        let c6 = Card::new(Value::Six, Suit::Heart);
        let not1 = Card::new(Value::Seven, Suit::Heart);
        let not2 = Card::new(Value::Six, Suit::Diamond);

        // Valid 5 (2h, 3h, 4h, 5h ,6h)
        let hand = SelectHand::new(vec![c2, c3, c4, c5, c6]);
        let sf = hand.is_straight_flush();
        assert_eq!(sf.unwrap().len(), 5);

        // Valid 5 with low ace (Ah, 2h, 3h, 4h, 5h)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, c5]);
        let sf = hand.is_straight_flush();
        assert_eq!(sf.unwrap().len(), 5);

        // Invalid 5, wrong value (2h, 3h, 4h, 5h, 7h)
        let hand = SelectHand::new(vec![c2, c3, c4, c5, not1]);
        let sf = hand.is_straight_flush();
        assert_eq!(sf, None);

        // Invalid 5, wrong suit (2h, 3h, 4h, 5h, 6d)
        let hand = SelectHand::new(vec![c2, c3, c4, c5, not2]);
        let sf = hand.is_straight_flush();
        assert_eq!(sf, None);

        // Invalid 4 (2h, 3h, 4h, 5h)
        let hand = SelectHand::new(vec![c2, c3, c4, c5]);
        let sf = hand.is_straight_flush();
        assert_eq!(sf, None);
    }

    #[test]
    fn test_royal_flush() {
        let c1 = Card::new(Value::Ten, Suit::Spade);
        let c2 = Card::new(Value::Jack, Suit::Spade);
        let c3 = Card::new(Value::Queen, Suit::Spade);
        let c4 = Card::new(Value::King, Suit::Spade);
        let c5 = Card::new(Value::Ace, Suit::Spade);
        let not1 = Card::new(Value::Nine, Suit::Spade);
        let not2 = Card::new(Value::Ace, Suit::Diamond);

        // Valid 5 (10s, Js, Qs, Ks, As)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, c5]);
        let rf = hand.is_royal_flush();
        assert_eq!(rf.unwrap().len(), 5);

        // Valid 5, scrambled input order (Js, 10s, Ks, Qs, As)
        let hand = SelectHand::new(vec![c2, c1, c4, c3, c5]);
        let rf = hand.is_royal_flush();
        assert_eq!(rf.unwrap().len(), 5);

        // Invalid 5, wrong value (9s, Js, Qs, Ks, As)
        let hand = SelectHand::new(vec![not1, c2, c3, c4, c5]);
        let rf = hand.is_royal_flush();
        assert_eq!(rf, None);

        // Invalid 5, wrong suit (10s, Js, Qs, Ks, Ad)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, not2]);
        let rf = hand.is_royal_flush();
        assert_eq!(rf, None);

        // Invalid 4 (2h, 3h, 4h, 5h)
        let hand = SelectHand::new(vec![c2, c3, c4, c5]);
        let rf = hand.is_royal_flush();
        assert_eq!(rf, None);
    }

    #[test]
    fn test_five_of_kind() {
        let c1 = Card::new(Value::King, Suit::Heart);
        let c2 = Card::new(Value::King, Suit::Spade);
        let c3 = Card::new(Value::King, Suit::Heart);
        let c4 = Card::new(Value::King, Suit::Diamond);
        let c5 = Card::new(Value::King, Suit::Heart);
        let not = Card::new(Value::Ace, Suit::Heart);

        // Valid 5 (K, K, K, K, K)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, c5]);
        let is_5 = hand.is_five_of_kind();
        assert_eq!(is_5.unwrap().len(), 5);

        // Valid 5 from 7 cards (K, K, K, K, K, A, A)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, c5, not, not]);
        let is_5 = hand.is_five_of_kind();
        assert_eq!(is_5.unwrap().len(), 5);

        // Invalid 5 (K, K, K, K, A)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, not]);
        let is_5 = hand.is_five_of_kind();
        assert_eq!(is_5, None);

        // Invalid 4 (K, K, K, K)
        let hand = SelectHand::new(vec![c1, c2, c3, c4]);
        let is_5 = hand.is_five_of_kind();
        assert_eq!(is_5, None);
    }

    #[test]
    fn test_five_of_kind_preserves_order() {
        let k1 = Card::new(Value::King, Suit::Heart);
        let k2 = Card::new(Value::King, Suit::Spade);
        let k3 = Card::new(Value::King, Suit::Diamond);
        let k4 = Card::new(Value::King, Suit::Club);
        let k5 = Card::new(Value::King, Suit::Heart);
        let ah = Card::new(Value::Ace, Suit::Heart);

        let hand = SelectHand::new(vec![k1, ah, k2, k3, k4, k5]);
        let is_5 = hand.is_five_of_kind().unwrap();
        assert_eq!(is_5.cards(), vec![k1, k2, k3, k4, k5]);
    }

    #[test]
    fn test_flush_house() {
        let c1 = Card::new(Value::King, Suit::Heart);
        let c2 = Card::new(Value::King, Suit::Heart);
        let c3 = Card::new(Value::King, Suit::Heart);
        let c4 = Card::new(Value::Ace, Suit::Heart);
        let c5 = Card::new(Value::Ace, Suit::Heart);
        let not1 = Card::new(Value::Two, Suit::Heart);
        let not2 = Card::new(Value::Ace, Suit::Diamond);

        // Valid 5 (Kh, Kh, Kh, Ah, Ah)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, c5]);
        let fh = hand.is_flush_house();
        assert_eq!(fh.unwrap().len(), 5);

        // Invalid 5 (Kh, Kh, Kh, Ah, 2h)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, not1]);
        let fh = hand.is_flush_house();
        assert_eq!(fh, None);

        // Invalid 5 (Kh, Kh, Kh, Ah, Ad)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, not2]);
        let fh = hand.is_flush_house();
        assert_eq!(fh, None);

        // Invalid 4 (Kh, Kh, Kh, Ah)
        let hand = SelectHand::new(vec![c1, c2, c3, c4]);
        let fh = hand.is_flush_house();
        assert_eq!(fh, None);
    }

    #[test]
    fn test_flush_with_wild() {
        // 4 hearts + 1 Wild = flush
        let c1 = Card::new(Value::King, Suit::Heart);
        let c2 = Card::new(Value::Queen, Suit::Heart);
        let c3 = Card::new(Value::Jack, Suit::Heart);
        let c4 = Card::new(Value::Seven, Suit::Heart);
        let mut wild = Card::new(Value::Two, Suit::Diamond);
        wild.enhancement = Some(Enhancement::Wild);

        let hand = SelectHand::new(vec![c1, c2, c3, c4, wild]);
        assert!(hand.is_flush().is_some());
        assert_eq!(hand.is_flush().unwrap().len(), 5);
    }

    #[test]
    fn test_flush_wild_insufficient() {
        // 3 hearts + 1 diamond + 1 Wild = not a flush
        let c1 = Card::new(Value::King, Suit::Heart);
        let c2 = Card::new(Value::Queen, Suit::Heart);
        let c3 = Card::new(Value::Jack, Suit::Heart);
        let c4 = Card::new(Value::Ace, Suit::Diamond);
        let mut wild = Card::new(Value::Two, Suit::Club);
        wild.enhancement = Some(Enhancement::Wild);

        let hand = SelectHand::new(vec![c1, c2, c3, c4, wild]);
        assert!(hand.is_flush().is_none());
    }

    #[test]
    fn test_flush_two_wilds() {
        // 3 hearts + 2 Wilds = flush
        let c1 = Card::new(Value::King, Suit::Heart);
        let c2 = Card::new(Value::Queen, Suit::Heart);
        let c3 = Card::new(Value::Jack, Suit::Heart);
        let mut wild1 = Card::new(Value::Two, Suit::Diamond);
        wild1.enhancement = Some(Enhancement::Wild);
        let mut wild2 = Card::new(Value::Three, Suit::Spade);
        wild2.enhancement = Some(Enhancement::Wild);

        let hand = SelectHand::new(vec![c1, c2, c3, wild1, wild2]);
        assert!(hand.is_flush().is_some());
    }

    #[test]
    fn test_flush_five() {
        let c1 = Card::new(Value::King, Suit::Heart);
        let c2 = Card::new(Value::King, Suit::Heart);
        let c3 = Card::new(Value::King, Suit::Heart);
        let c4 = Card::new(Value::King, Suit::Heart);
        let c5 = Card::new(Value::King, Suit::Heart);
        let not1 = Card::new(Value::Two, Suit::Heart);
        let not2 = Card::new(Value::King, Suit::Diamond);

        // Valid 5 (Kh, Kh, Kh, Kh, Kh)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, c5]);
        let ff = hand.is_flush_five();
        assert_eq!(ff.unwrap().len(), 5);

        // Invalid 5 (Kh, Kh, Kh, Kh, 2h)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, not1]);
        let ff = hand.is_flush_five();
        assert_eq!(ff, None);

        // Invalid 5 (Kh, Kh, Kh, Kh, Kd)
        let hand = SelectHand::new(vec![c1, c2, c3, c4, not2]);
        let ff = hand.is_flush_five();
        assert_eq!(ff, None);

        // Invalid 4 (Kh, Kh, Kh, Kh)
        let hand = SelectHand::new(vec![c1, c2, c3, c4]);
        let ff = hand.is_flush_five();
        assert_eq!(ff, None);
    }

    #[test]
    fn test_stone_cards_have_no_rank_or_suit() {
        let ace = Card::new(Value::Ace, Suit::Heart);
        let king = Card::new(Value::King, Suit::Heart);
        let queen = Card::new(Value::Queen, Suit::Heart);
        let jack = Card::new(Value::Jack, Suit::Heart);
        let mut stone = Card::new(Value::Ten, Suit::Heart);
        stone.enhancement = Some(Enhancement::Stone);

        // 4 suited cards + a Stone do not make a flush.
        let hand = SelectHand::new(vec![ace, king, queen, jack, stone]);
        assert!(hand.is_flush().is_none());

        // A K Q J + Stone is not a straight; best is High Card Ace.
        let best = hand.best_hand().unwrap();
        assert_eq!(best.rank, HandRank::HighCard);
        assert_eq!(best.hand.cards(), vec![ace]);

        // Two Kings + a Stone King is only a pair, not trips.
        let k1 = Card::new(Value::King, Suit::Club);
        let k2 = Card::new(Value::King, Suit::Diamond);
        let mut stone_king = Card::new(Value::King, Suit::Spade);
        stone_king.enhancement = Some(Enhancement::Stone);
        let best = SelectHand::new(vec![k1, k2, stone_king])
            .best_hand()
            .unwrap();
        assert_eq!(best.rank, HandRank::OnePair);
        assert_eq!(best.hand.cards().len(), 2);

        // A Stone-only hand is High Card with an empty scoring subset; the
        // kicker scoring pass handles the +50 chips.
        let best = SelectHand::new(vec![stone]).best_hand().unwrap();
        assert_eq!(best.rank, HandRank::HighCard);
        assert!(best.hand.cards().is_empty());
    }

    #[test]
    fn test_four_fingers_four_card_flush_and_straight() {
        let flush = vec![
            Card::new(Value::Ace, Suit::Heart),
            Card::new(Value::King, Suit::Heart),
            Card::new(Value::Queen, Suit::Heart),
            Card::new(Value::Nine, Suit::Heart),
        ];
        let rules = HandRules {
            four_fingers: true,
            ..Default::default()
        };
        let best = SelectHand::new(flush).best_hand_with_rules(rules).unwrap();
        assert_eq!(best.rank, HandRank::Flush);
        assert_eq!(best.hand.len(), 4);

        let straight = vec![
            Card::new(Value::Two, Suit::Heart),
            Card::new(Value::Three, Suit::Diamond),
            Card::new(Value::Four, Suit::Club),
            Card::new(Value::Five, Suit::Spade),
        ];
        let best = SelectHand::new(straight)
            .best_hand_with_rules(rules)
            .unwrap();
        assert_eq!(best.rank, HandRank::Straight);
        assert_eq!(best.hand.len(), 4);
    }

    #[test]
    fn test_shortcut_allows_one_rank_gap() {
        let cards = vec![
            Card::new(Value::Two, Suit::Heart),
            Card::new(Value::Three, Suit::Diamond),
            Card::new(Value::Five, Suit::Club),
            Card::new(Value::Six, Suit::Spade),
            Card::new(Value::Seven, Suit::Heart),
        ];
        let rules = HandRules {
            shortcut: true,
            ..Default::default()
        };
        let best = SelectHand::new(cards.clone())
            .best_hand_with_rules(rules)
            .unwrap();
        assert_eq!(best.rank, HandRank::Straight);
        assert_eq!(best.hand.len(), 5);

        let normal = SelectHand::new(cards).best_hand().unwrap();
        assert_ne!(normal.rank, HandRank::Straight);
    }

    #[test]
    fn test_smeared_joker_makes_red_flush() {
        let cards = vec![
            Card::new(Value::Ace, Suit::Heart),
            Card::new(Value::King, Suit::Diamond),
            Card::new(Value::Queen, Suit::Heart),
            Card::new(Value::Jack, Suit::Diamond),
            Card::new(Value::Nine, Suit::Heart),
        ];
        let rules = HandRules {
            smeared: true,
            ..Default::default()
        };
        let best = SelectHand::new(cards.clone())
            .best_hand_with_rules(rules)
            .unwrap();
        assert_eq!(best.rank, HandRank::Flush);
        assert_eq!(best.hand.len(), 5);

        let normal = SelectHand::new(cards).best_hand().unwrap();
        assert_ne!(normal.rank, HandRank::Flush);
    }
}
