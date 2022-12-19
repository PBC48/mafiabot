mod error {
    use std::error::Error;
    use std::fmt::Display;

    #[derive(Debug, Clone)]
    pub struct ValidationErr {
        pub msg: String,
    }

    impl ValidationErr {
        pub fn new(msg: &str) -> Self {
            Self {
                msg: msg.to_string(),
            }
        }
    }

    impl Display for ValidationErr {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.msg)
        }
    }

    impl Error for ValidationErr {}
}

mod interface {
    use serde::{Deserialize, Serialize};
    use std::fmt::Debug;

    use super::game::{Actor, Ballot, Phase, Pidx, Player, RawPID, Target};
    // Eventually this will require a way to respond?
    pub trait Source: Debug + Clone + Default + Send {}

    /// Has details about where the command came from
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Request<U: RawPID, S: Source> {
        pub cmd: Command<U>,
        pub src: S,
        // Implementation specifics
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Command<U: RawPID> {
        Vote(U, Option<Ballot<U>>),
        Action(Actor<U>, Option<Target<U>>),
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Response<U: RawPID, S: Source> {
        pub event: Event<U>,
        pub src: S,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Event<U: RawPID> {
        Start {
            players: Vec<Player<U>>,
            phase: Phase,
        },
        Day,
        Vote {
            voter: Pidx,
            ballot: Option<Ballot<Pidx>>,
            former: Option<Ballot<Pidx>>,
            threshold: usize,
            count: usize,
        },
        Elect {
            ballot: Ballot<Pidx>,
        },
        Night,
        Action {
            actor: Actor<Pidx>,
            target: Option<Target<Pidx>>,
        },
        Dawn,
        Strip,
        Save,
        Investigate,
        Kill,
        Eliminate {
            player: Pidx,
        },
        Win,
        End,
        InvalidCommand,
    }
}

mod role {
    use super::game::RawPID;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize /*Deserialize*/)]
    pub enum Role<U: RawPID> {
        TOWN,
        COP,
        DOCTOR,
        CELEB,
        MILLER,
        MASON,
        MAFIA,
        GODFATHER,
        STRIPPER,
        GOON,
        IDIOT,
        SURVIVOR,
        GUARD(U),
        AGENT(U),
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize /*Deserialize*/)]
    pub enum Team {
        Town,
        Mafia,
        Rogue,
    }
    impl<U: RawPID> Role<U> {
        pub fn team(&self) -> Team {
            match self {
                Role::TOWN | Role::COP | Role::DOCTOR | Role::CELEB => Team::Town,
                Role::MILLER | Role::MASON => Team::Town,
                Role::MAFIA | Role::GODFATHER | Role::GOON | Role::STRIPPER => Team::Mafia,
                Role::IDIOT | Role::SURVIVOR | Role::GUARD(_) | Role::AGENT(_) => Team::Rogue,
            }
        }
        pub fn investigate_mafia(&self) -> bool {
            match self {
                Role::GODFATHER => false,
                Role::MILLER => true,
                _ => self.team() == Team::Mafia,
            }
        }

        pub fn has_night_action(&self) -> bool {
            match self {
                Role::COP | Role::DOCTOR | Role::STRIPPER => true,
                _ => false,
            }
        }
    }
}

mod game {
    use serde::{Deserialize, Serialize};
    use std::fmt::{Debug, Display};
    use std::{
        sync::mpsc::{Receiver, Sender},
        thread::{self, JoinHandle},
    };

    use super::interface::Event;
    use super::role::Role;
    use super::{
        error::ValidationErr,
        interface::{Command, Request, Response, Source},
        role::Team,
    };

    pub trait RawPID: Debug + Display + Clone + Copy + PartialEq + Eq + Send + Serialize {}

    pub type Pidx = usize;
    impl RawPID for Pidx {}

    #[derive(Debug, Clone, PartialEq, Eq, Serialize /*Deserialize*/)]
    pub struct Player<U: RawPID> {
        pub raw_pid: U,
        pub name: String,
        pub role: Role<U>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize /*Deserialize*/)]
    pub enum Winner {
        Team(Team),
        Player(Pidx),
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize /*Deserialize*/)]
    pub enum Ballot<U: RawPID> {
        Player(U),
        Abstain,
    }

    impl<U: RawPID> Display for Ballot<U> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Ballot::Player(p) => write!(f, "Player({})", p),
                Ballot::Abstain => write!(f, "Abstain"),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize /*Deserialize*/)]
    pub enum Actor<U: RawPID> {
        Player(U),
        Mafia(U),
    }
    impl<U: RawPID> Actor<U> {
        fn overlaps(&self, other: &Self) -> bool {
            match (self, other) {
                (Actor::Player(p1), Actor::Player(p2)) => p1 == p2,
                (Actor::Mafia(_), Actor::Mafia(_)) => true,
                _ => false,
            }
        }
        fn is_player(&self, p: U) -> bool {
            match self {
                Actor::Player(p2) => p == *p2,
                _ => false,
            }
        }
        fn is_mafia(&self) -> bool {
            match self {
                Actor::Mafia(_) => true,
                _ => false,
            }
        }
    }
    impl<U: RawPID> Display for Actor<U> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Actor::Player(p) => write!(f, "Player({})", p),
                Actor::Mafia(p) => write!(f, "Mafia({})", p),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize /*Deserialize*/)]
    pub enum Target<U: RawPID> {
        Player(U),
        NoTarget,
        Blocked,
    }

    pub type Votes = Vec<(Pidx, Ballot<Pidx>)>;
    pub type Actions = Vec<(Actor<Pidx>, Target<Pidx>)>;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize /*Deserialize*/)]
    pub enum Phase {
        Init,
        Day {
            day_no: usize,
            #[serde(skip)]
            votes: Votes,
        },
        Night {
            night_no: usize,
            #[serde(skip)]
            actions: Actions,
        },
        End(Winner),
    }

    impl Phase {
        pub fn clear(&mut self) {
            match self {
                Phase::Day { votes, .. } => votes.clear(),
                Phase::Night { actions, .. } => actions.clear(),
                _ => {}
            }
        }
    }

    // Want to ensure players can't be modified without clearing phase...
    type Players<U> = Vec<Player<U>>;

    #[derive(Debug, Clone, Serialize /*Deserialize*/)]
    pub struct Game<U: RawPID, S: Source> {
        players: Players<U>,
        phase: Phase,
        _pd: std::marker::PhantomData<S>,
    }

    impl<U: RawPID, S: Source> Game<U, S> {
        pub fn new(players: Players<U>) -> Self {
            let mut game = Self {
                players: Vec::new(),
                phase: Phase::Init,
                _pd: std::marker::PhantomData,
            };
            for player in players {
                // Todo print errors?
                Self::add_player(&mut game.players, &mut game.phase, player);
            }
            return game;
        }

        pub fn add_player(
            players: &mut Players<U>,
            phase: &Phase,
            player: Player<U>,
        ) -> Result<(), ValidationErr> {
            if let Phase::Init = phase {
                if Self::check_player(&players, &player.raw_pid).is_ok() {
                    return Err(ValidationErr::new("Player already exists"));
                }
                players.push(player);
                Ok(())
            } else {
                return Err(ValidationErr::new("Can't add player during game"));
            }
        }

        pub fn check_player(players: &Players<U>, raw_pid: &U) -> Result<Pidx, ValidationErr> {
            players
                .iter()
                .position(|p| p.raw_pid == *raw_pid)
                .map(|i: Pidx| i)
                .ok_or_else(|| ValidationErr {
                    msg: format!("Player {:?} not found", raw_pid),
                })
        }

        pub fn get_players_that(
            players: &Players<U>,
            f: fn((Pidx, Player<U>)) -> bool,
        ) -> impl Iterator<Item = (Pidx, Player<U>)> + '_ {
            players
                .iter()
                .enumerate()
                .map(|(i, p)| (i, p.clone()))
                .filter(move |(i, p)| f((*i, p.clone())))
        }
    }
    impl<U: RawPID + 'static, S: 'static + Source> Game<U, S> {
        pub fn start(
            mut self,
            rx: Receiver<Request<U, S>>,
            tx: Sender<Response<U, S>>,
        ) -> JoinHandle<()> {
            // Start game thread
            thread::spawn(move || self.game_thread(rx, tx))
        }
    }
    impl<U: RawPID, S: Source> Game<U, S> {
        fn game_thread(&mut self, rx: Receiver<Request<U, S>>, tx: Sender<Response<U, S>>) {
            Self::next_phase(&mut self.players, &mut self.phase, &tx, &S::default());
            tx.send(Response {
                event: Event::Start {
                    players: self.players.clone(),
                    phase: self.phase.clone(),
                },
                src: S::default(),
            })
            .unwrap();

            loop {
                println!("Serialize: {}", serde_json::to_string(&self).unwrap());
                match &mut self.phase {
                    Phase::Day {
                        day_no,
                        ref mut votes,
                    } => {
                        if let Ok(Request {
                            cmd: Command::Vote(raw_voter, raw_ballot),
                            src,
                        }) = rx.recv()
                        {
                            let elect = Self::handle_vote(
                                &mut self.players,
                                votes,
                                raw_voter,
                                raw_ballot,
                                &tx,
                                &src,
                            );
                            match elect {
                                None => {}
                                Some(ballot) => {
                                    // "elect" subfn
                                    Self::handle_elect(
                                        &mut self.players,
                                        &mut self.phase,
                                        ballot,
                                        &tx,
                                        &src,
                                    );
                                }
                            }
                        }
                    }

                    Phase::Night {
                        night_no,
                        ref mut actions,
                    } => {
                        if let Ok(Request {
                            cmd: Command::Action(raw_actor, raw_target),
                            src,
                        }) = rx.recv()
                        {
                            if Self::handle_action(
                                &mut self.players,
                                actions,
                                raw_actor,
                                raw_target,
                                &tx,
                                &src,
                            ) {
                                let victim =
                                    Self::handle_dawn(&mut self.players, actions, &tx, &src);

                                match victim {
                                    None => {}
                                    Some(victim) => {
                                        Self::eliminate(
                                            &mut self.players,
                                            &mut self.phase,
                                            victim,
                                            &tx,
                                            &src,
                                        );
                                    }
                                };
                                Self::next_phase(&self.players, &mut self.phase, &tx, &src);
                            }
                        }
                    }
                    Phase::End(winner) => {
                        break;
                    }
                    _ => {
                        tx.send(Response {
                            event: Event::End,
                            src: S::default(),
                        })
                        .unwrap();
                    }
                };
            }
            // Ok(())
        }

        fn handle_vote(
            players: &mut Players<U>,
            votes: &mut Votes,
            raw_voter: U,
            raw_ballot: Option<Ballot<U>>,
            tx: &Sender<Response<U, S>>,
            src: &S,
        ) -> Option<Ballot<Pidx>> {
            match Self::validate_vote(players, raw_voter, raw_ballot, &tx, src) {
                Err(err) => {
                    // Handle error response
                    tx.send(Response {
                        event: Event::InvalidCommand,
                        src: src.clone(),
                    })
                    .unwrap();
                    None
                }
                Ok((voter, ballot)) => {
                    let former = Self::accept_vote(votes, voter, ballot, tx, src);
                    Self::check_elect(players, votes, former, tx, src)
                }
            }
        }

        fn validate_vote(
            players: &Players<U>,
            raw_voter: U,
            raw_ballot: Option<Ballot<U>>,
            tx: &Sender<Response<U, S>>,
            src: &S,
        ) -> Result<(Pidx, Option<Ballot<Pidx>>), ValidationErr> {
            let voter = Self::check_player(players, &raw_voter)?;
            let ballot = match raw_ballot {
                Some(Ballot::Player(raw_pid)) => {
                    Some(Ballot::Player(Self::check_player(players, &raw_pid)?))
                }
                Some(Ballot::Abstain) => Some(Ballot::Abstain),
                None => None,
            };
            Ok((voter, ballot))
        }

        fn accept_vote(
            votes: &mut Votes,
            voter: Pidx,
            ballot: Option<Ballot<Pidx>>,
            tx: &Sender<Response<U, S>>,
            src: &S,
        ) -> Option<Ballot<Pidx>> {
            let former = votes
                .iter()
                .position(|(v, _)| v == &voter)
                .map(|i| votes.remove(i));
            if let Some(ballot) = ballot {
                println!("Player {} votes for {:?}", voter, ballot);
                votes.push((voter, ballot));
            }
            former.map(|(v, b)| b)
        }

        fn check_elect(
            players: &Players<U>,
            votes: &Votes,
            former: Option<Ballot<Pidx>>,
            tx: &Sender<Response<U, S>>,
            src: &S,
        ) -> Option<Ballot<Pidx>> {
            let n_players = players.len();
            let threshold = n_players / 2 + 1;
            let lo_thresh = (n_players + 1) / 2;

            if votes.len() == 0 {
                return None;
            }
            let (last_voter, last_ballot) = votes.last().unwrap();

            let threshold = match last_ballot {
                Ballot::Abstain => lo_thresh,
                _ => threshold,
            };

            let count = votes.iter().filter(|(_, b)| b == last_ballot).count();
            tx.send(Response {
                event: Event::Vote {
                    voter: *last_voter,
                    ballot: Some(*last_ballot),
                    former,
                    count,
                    threshold,
                },
                src: src.clone(),
            })
            .unwrap();
            match last_ballot {
                Ballot::Player(candidate) if count >= threshold => Some(Ballot::Player(*candidate)),
                Ballot::Abstain if count >= lo_thresh => Some(Ballot::Abstain),
                _ => None,
            }
        }

        fn handle_elect(
            players: &mut Players<U>,
            phase: &mut Phase,
            ballot: Ballot<Pidx>,
            tx: &Sender<Response<U, S>>,
            src: &S,
        ) {
            println!("ELECTED: {:?}", ballot);
            tx.send(Response {
                event: Event::Elect { ballot },
                src: src.clone(),
            })
            .unwrap();
            match ballot {
                Ballot::Player(elect) => {
                    Self::eliminate(players, phase, elect, tx, src);
                }
                Ballot::Abstain => {}
            };
            Self::next_phase(&players, phase, tx, src);
        }

        fn handle_action(
            players: &mut Players<U>,
            actions: &mut Actions,
            raw_actor: Actor<U>,
            raw_target: Option<Target<U>>,
            tx: &Sender<Response<U, S>>,
            src: &S,
        ) -> bool {
            match Self::validate_action(players, raw_actor, raw_target, tx, src) {
                Err(err) => {
                    // Handle error response
                    tx.send(Response {
                        event: Event::InvalidCommand,
                        src: src.clone(),
                    })
                    .unwrap();
                    false
                }
                Ok((actor, target)) => {
                    Self::accept_action(actions, actor, target, tx, src);
                    Self::check_dawn(players, actions, tx, src)
                }
            }
        }

        fn validate_action(
            players: &Players<U>,
            raw_actor: Actor<U>,
            raw_target: Option<Target<U>>,
            tx: &Sender<Response<U, S>>,
            src: &S,
        ) -> Result<(Actor<Pidx>, Option<Target<Pidx>>), ValidationErr> {
            let actor = match raw_actor {
                Actor::Player(raw_pid) => Actor::Player(Self::check_player(players, &raw_pid)?),
                Actor::Mafia(raw_pid) => Actor::Mafia(Self::check_player(players, &raw_pid)?),
            };
            let target = match raw_target {
                Some(Target::Player(raw_pid)) => {
                    Some(Target::Player(Self::check_player(players, &raw_pid)?))
                }
                Some(Target::NoTarget) => Some(Target::NoTarget),

                None | Some(Target::Blocked) => None,
            };
            Ok((actor, target))
        }

        fn accept_action(
            actions: &mut Actions,
            actor: Actor<Pidx>,
            target: Option<Target<Pidx>>,
            tx: &Sender<Response<U, S>>,
            src: &S,
        ) {
            // TODO: Role Check? Goon -> Target::Blocked?
            let former = actions
                .iter()
                .position(|(a, _)| a.overlaps(&actor))
                .map(|i| actions.remove(i));
            if let Some(target) = target {
                println!("Player {} acts on {:?}", actor, target);
                tx.send(Response {
                    event: Event::Action {
                        actor,
                        target: Some(target),
                    },
                    src: src.clone(),
                })
                .unwrap();
                actions.push((actor, target));
            }
        }

        fn check_dawn(
            players: &Players<U>,
            actions: &Actions,
            tx: &Sender<Response<U, S>>,
            src: &S,
        ) -> bool {
            // Check that all possible actors have acted
            let actors = players
                .iter()
                .enumerate()
                .filter(|(_, p)| p.role.has_night_action())
                .map(|(i, _)| Actor::Player(i))
                .chain([Actor::Mafia(0)])
                .collect::<Vec<_>>();

            // For all actors, check that they have acted, or if Mafia, that at least one has acted
            for actor in actors {
                match actor {
                    Actor::Player(pid) => {
                        if actions.iter().find(|(a, _)| a == &actor).is_none() {
                            return false;
                        }
                    }
                    Actor::Mafia(_) => {
                        if actions.iter().find(|(a, _)| a.is_mafia()).is_none() {
                            return false;
                        }
                    }
                }
            }
            true
        }

        fn handle_dawn(
            players: &Players<U>,
            actions: &mut Actions,
            tx: &Sender<Response<U, S>>,
            src: &S,
        ) -> Option<Pidx> {
            tx.send(Response {
                event: Event::Dawn,
                src: src.clone(),
            })
            .unwrap();
            // Strip
            Self::get_players_that(players, |(_, p)| p.role == Role::STRIPPER)
                .for_each(|(stripped, _)| Self::strip(actions, stripped, tx, src));

            Self::get_players_that(players, |(_, p)| p.role == Role::DOCTOR)
                .for_each(|(saved, _)| Self::save(actions, saved, tx, src));

            let cops = Self::get_players_that(players, |(_, p)| p.role == Role::COP);
            for (cop, _) in cops {
                let suspect = actions
                    .iter()
                    .find(|(a, _)| a.is_player(cop))
                    .map(|(_, t)| t);
                if let Some(Target::Player(suspect)) = suspect {
                    Self::investigate(cop, *suspect, players, &tx, src)
                }
            }

            let kill = actions.iter().find(|(a, _)| a.is_mafia());
            match kill {
                Some((a, Target::Player(victim))) => {
                    tx.send(Response {
                        event: Event::Kill,
                        src: src.clone(),
                    })
                    .unwrap();
                    Some(*victim)
                }
                _ => None,
            }
        }

        fn eliminate(
            players: &mut Players<U>,
            phase: &mut Phase,
            victim: Pidx,
            tx: &Sender<Response<U, S>>,
            src: &S,
        ) {
            tx.send(Response {
                event: Event::Eliminate { player: victim },
                src: src.clone(),
            })
            .unwrap();
            println!("Eliminating player {}", victim);
            players.remove(victim);
            phase.clear();
            match Self::check_win(players, &tx, src) {
                None => {}
                Some(winner) => {
                    *phase = Phase::End(Winner::Team(winner));
                }
            }
        }

        fn next_phase(
            players: &Players<U>,
            phase: &mut Phase,
            tx: &Sender<Response<U, S>>,
            src: &S,
        ) {
            match phase {
                Phase::Init => {
                    // TODO: set phase based on rules
                    *phase = Phase::Day {
                        day_no: 1,
                        votes: Vec::new(),
                    };
                }
                Phase::Day { day_no, .. } => {
                    println!("Day {} ends", day_no);
                    *phase = Phase::Night {
                        night_no: *day_no,
                        actions: Vec::new(),
                    };
                }
                Phase::Night { night_no, .. } => {
                    println!("Night {} ends", night_no);
                    *phase = Phase::Day {
                        day_no: *night_no + 1,
                        votes: Vec::new(),
                    };
                }
                _ => {}
            };
            match phase {
                Phase::Day { .. } => {
                    tx.send(Response {
                        event: Event::Day,
                        src: src.clone(),
                    })
                    .unwrap();
                }
                Phase::Night { .. } => {
                    tx.send(Response {
                        event: Event::Night,
                        src: src.clone(),
                    })
                    .unwrap();
                }
                Phase::End(_) => {
                    tx.send(Response {
                        event: Event::End,
                        src: src.clone(),
                    })
                    .unwrap();
                }
                Phase::Init => {
                    panic!("Shouldn't ever next phase into Init")
                }
            };
        }

        fn check_win(players: &Players<U>, tx: &Sender<Response<U, S>>, src: &S) -> Option<Team> {
            let n_players = players.len();
            let n_mafia = players
                .iter()
                .filter(|p| p.role.team() == Team::Mafia)
                .count();
            let result = match 0 {
                _ if n_mafia == 0 => Some(Team::Town),
                _ if n_players <= n_mafia * 2 => Some(Team::Mafia),
                _ => None,
            };
            if result.is_some() {
                tx.send(Response {
                    event: Event::Win,
                    src: src.clone(),
                })
                .unwrap();
            }
            println!("Win condition: {:?}", result);
            result
        }

        fn strip(actions: &mut Actions, stripped: Pidx, tx: &Sender<Response<U, S>>, src: &S) {
            for (actor, target) in actions {
                if actor == &Actor::Player(stripped) {
                    *target = Target::Blocked;

                    tx.send(Response {
                        event: Event::Strip,
                        src: src.clone(),
                    })
                    .unwrap();
                }
            }
        }

        fn save(actions: &mut Actions, saved: Pidx, tx: &Sender<Response<U, S>>, src: &S) {
            for (actor, target) in actions {
                if let Actor::Mafia(_) = actor {
                    *target = match target {
                        Target::Player(pid) if *pid == saved => Target::Blocked,
                        _ => *target,
                    };

                    tx.send(Response {
                        event: Event::Save,
                        src: src.clone(),
                    })
                    .unwrap();
                }
            }
        }

        fn investigate(
            cop: Pidx,
            suspect: Pidx,
            players: &Players<U>,
            tx: &Sender<Response<U, S>>,
            src: &S,
        ) {
            let is_mafia = players[suspect].role.investigate_mafia();

            tx.send(Response {
                event: Event::Investigate,
                src: src.clone(),
            })
            .unwrap();
            // println!("Cop {:?} investigates {:?} and finds {:?}", cop, suspect, is_mafia);
        }
    }
}

mod test {
    use super::error::*;
    use super::game::*;
    use super::interface::*;
    use super::role::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn minimal() {
        impl RawPID for u64 {}
        impl Source for String {}

        let mut players = vec![
            Player {
                raw_pid: 1u64,
                name: "p1".to_string(),
                role: Role::TOWN,
            },
            Player {
                raw_pid: 2u64,
                name: "p2".to_string(),
                role: Role::TOWN,
            },
            Player {
                raw_pid: 3u64,
                name: "p3".to_string(),
                role: Role::MAFIA,
            },
        ];

        let (send_cmd, rx) = mpsc::channel::<Request<u64, String>>();
        let (tx, recv_event) = mpsc::channel::<Response<u64, String>>();
        let game = Game::new(players);
        let g: thread::JoinHandle<()> = game.start(rx, tx);
        send_cmd
            .send(Request {
                src: "vote1".to_string(),
                cmd: Command::Vote(1u64, Some(Ballot::Player(3u64))),
            })
            .unwrap();

        send_cmd
            .send(Request {
                src: "vote2".to_string(),
                cmd: Command::Vote(2u64, Some(Ballot::Player(3u64))),
            })
            .unwrap();

        // sleep .5 seconds
        thread::sleep(Duration::from_millis(200));

        assert!(g.is_finished());
    }
}
// send_cmd
//     .send(Request {
//         src: "vote1".to_string(),
//         cmd: Command::Vote(1u64, Some(Ballot::Player(3u64))),
//     })
//     .unwrap();

// send_cmd
//     .send(Request {
//         src: "vote2".to_string(),
//         cmd: Command::Vote(2u64, Some(Ballot::Player(3u64))),
//     })
//     .unwrap();

// // sleep .5 seconds
// thread::sleep(Duration::from_millis(200));

// assert!(g.is_finished());

//         // Look at events
//         let mut response_iter = recv_event.try_iter();

//         response_iter.for_each(|r| {
//             println!("{:#?}", r);
//         });

//         // let event = response_iter.next();
//         // match event {
//         //     Some(Response {
//         //         event:
//         //             Event::Start {
//         //                 phase: Phase::Day(1, _),
//         //                 ..
//         //             },
//         //         ..
//         //     }) => {
//         //         dbg!(event);
//         //     }
//         //     _ => {
//         //         assert!(false, "Unexpected event: {:?}", event);
//         //     }
//         // }

//         // let event = response_iter.next();
//         // match event {
//         //     Some(Response {
//         //         event:
//         //             Event::Vote {
//         //                 voter: 0,
//         //                 ballot: Some(Ballot::Player(2)),
//         //                 former: None,
//         //                 threshold: 2,
//         //                 count: 1,
//         //             },
//         //         ..
//         //     }) => {
//         //         dbg!(event);
//         //     }
//         //     _ => {
//         //         assert!(false, "Unexpected event: {:?}", event);
//         //     }
//         // }
//         // let event = response_iter.next();
//         // match event {
//         //     Some(Response {
//         //         event:
//         //             Event::Vote {
//         //                 voter: 1,
//         //                 ballot: Some(Ballot::Player(2)),
//         //                 former: None,
//         //                 threshold: 2,
//         //                 count: 2,
//         //             },
//         //         ..
//         //     }) => {
//         //         dbg!(event);
//         //     }
//         //     _ => {
//         //         assert!(false, "Unexpected event: {:?}", event);
//         //     }
//         // }
//         // let event = response_iter.next();
//         // match event {
//         //     Some(Response {
//         //         event:
//         //             Event::Elect {
//         //                 ballot: Ballot::Player(2),
//         //                 ..
//         //             },
//         //         ..
//         //     }) => {
//         //         dbg!(event);
//         //     }
//         //     _ => {
//         //         assert!(false, "Unexpected event: {:?}", event);
//         //     }
//         // }
//         // let event = response_iter.next();
//         // match event {
//         //     Some(Response {
//         //         event: Event::Eliminate { .. },
//         //         ..
//         //     }) => {
//         //         dbg!(event);
//         //     }
//         //     _ => {
//         //         assert!(false, "Unexpected event: {:?}", event);
//         //     }
//         // }
//     }
// }
