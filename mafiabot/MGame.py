import time

from .MInfo import *
from .MState import MState
from .MEvent import MPhase
from .MRules import RULE_BOOK

# TODO: Can MGame be folded into MState?
# TODO: Unique ID

# TODO: timer
# TODO: help

# Contains MState, checks inputs, fulfills non-event actions
class MGame:

  def __init__(self, MChatType, dms, rules, end_callback, users, roleGen):
    self.main_chat = MChatType.new("MAIN CHAT")
    self.mafia_chat = MChatType.new("MAFIA CHAT")
    self.dms = dms
    self.rules = rules

    def main_cast(msg:str):
      self.main_chat.cast(self.main_chat.format(msg))
    def mafia_cast(msg:str):
      self.mafia_chat.cast(self.main_chat.format(msg))
    def send_dm(msg:str,user_id):
      self.dms.send(self.main_chat.format(msg), user_id)
    def end_callback_(e):
      end_callback(self, e)

    self.main_cast = main_cast
    self.mafia_cast = mafia_cast
    self.send_dm = send_dm

    ids = list(users.keys())
    (ids, roles, contracts) = roleGen(ids)
    mafia_users = {}
    for id, role in zip(ids,roles):
      if role in MAFIA_ROLES:
        mafia_users[id] = users[id]

    self.start_roles = createStartRoles(ids, roles, contracts)

    self.main_chat.refill(users)
    self.mafia_chat.refill(mafia_users)
    self.state = MState(main_cast, mafia_cast, send_dm, self.rules, end_callback_, ids, roles, contracts)

  def active(self):
    return self.state.active

  def main_id(self):
    return self.main_chat.id

  def mafia_id(self):
    return self.mafia_chat.id

  def handle_main(self, sender_id, command, text, data):
    if command == VOTE_CMD:
      words = text.split()
      voter = sender_id
      votee = None
      if len(words) >= 2:
        # TODO: Generalize language
        if words[1].lower() == "me":
          votee = sender_id
        elif words[1].lower() == "none":
          votee = None
        elif words[1].lower() == "nokill":
          votee = "NOTARGET"
        elif 'attachments' in data:
          mentions = [a for a in data['attachments'] if a['type'] == 'mentions']
          if len(mentions) > 0 and 'user_ids' in mentions[0] and len(mentions[0]['user_ids']) >= 1:
            votee = mentions[0]['user_ids'][0]
      self.handle_vote(voter,votee)
    elif command == STATUS_CMD:
      self.handle_main_status()
    elif command == HELP_CMD:
      self.handle_main_help(text)
    elif command == TIMER_CMD:
      self.handle_timer(sender_id)
    elif command == UNTIMER_CMD:
      self.handle_untimer(sender_id)
    elif command == RULE_CMD:
      self.handle_rule("MAIN", text)

  def handle_mafia(self, sender_id, command, text, data):
    if command == TARGET_CMD:
      self.handle_mtarget(sender_id, text)
    elif command == STATUS_CMD:
      self.handle_mafia_status()
    elif command == HELP_CMD:
      self.handle_mafia_help(text)
    elif command == RULE_CMD:
      self.handle_rule("MAFIA",text)

  def handle_dm(self, sender_id, command, text, data):
    if command == TARGET_CMD:
      self.handle_target(sender_id, text)
    elif command == REVEAL_CMD:
      self.handle_reveal(sender_id)
    elif command == STATUS_CMD:
      self.handle_dm_status(sender_id)
    elif command == HELP_CMD:
      self.handle_dm_help(sender_id, text)
    elif command == RULE_CMD:
      self.handle_rule(sender_id,text)

  def handle_vote(self,player_id,target_id):
    if not player_id in self.state.players:
      self.main_cast(default_resp_lib["INVALID_VOTE_PLAYER"].format(player_id=player_id))
      return

    if not self.state.phase == MPhase.DAY:
      self.main_cast(default_resp_lib["INVALID_VOTE_PHASE"])
      return

    self.state.vote(player_id, target_id)
  
  @staticmethod
  def getTarget(text):
    words = text.split()
    target_letter = words[1]
    if not len(target_letter) == 1:
      raise TypeError()
    return target_letter

  def handle_target(self,player_id, text):
    if self.state.phase == MPhase.NIGHT:    
      if not (player_id in self.state.players and self.state.players[player_id].role in TARGETING_ROLES):
        self.send_dm(default_resp_lib["INVALID_TARGET_PLAYER"],player_id)
        return
    elif self.state.phase == MPhase.DUSK:
      if not (player_id in self.state.players and self.state.players[player_id].role == "IDIOT"):
        self.send_dm(default_resp_lib["INVALID_TARGET_PLAYER"],player_id)
        return
    else:
      self.send_dm(default_resp_lib["INVALID_TARGET_PHASE"],player_id)
      return

    try:
      target_letter = self.getTarget(text)
      target_number = ord(target_letter.upper())-ord('A')
      if self.state.phase == MPhase.DUSK:
        player_order = self.state.vengeance['venges']
      else:
        player_order = self.state.player_order
      if target_number == len(player_order):
        target_id = "NOTARGET"
      else:
        target_id = self.state.player_order[target_number]
    except Exception:
      self.send_dm(default_resp_lib["INVALID_TARGET"].format(text=text),player_id)
      return
    if (self.state.players[player_id].role == "MILKY" and 
        self.state.rules["no_milk_self"] == "ON" and
        target_id == player_id):
      self.send_dm(default_resp_lib["MILK_SELF"],player_id)
      return
    self.state.target(player_id, target_id)

  def handle_mtarget(self, player_id, text):
    if not (player_id in self.state.players and self.state.players[player_id].role in MAFIA_ROLES):
      self.mafia_cast(default_resp_lib["INVALID_MTARGET_PLAYER"])
      return

    if not self.state.phase == MPhase.NIGHT:
      self.mafia_cast(default_resp_lib["INVALID_MTARGET_PHASE"])
      return
    
    try:
      target_letter = self.getTarget(text)
      target_number = ord(target_letter.upper())-ord('A')
      if target_number == len(self.state.player_order):
        target_id = "NOTARGET"
      else:
        target_id = self.state.player_order[target_number]
    except Exception:
      self.mafia_cast(default_resp_lib["INVALID_MTARGET"].format(text=text))
      return
    
    role = self.state.players[player_id].role
    if role == "GOON" and target_id != "NOTARGET":
      self.mafia_cast(default_resp_lib["INVALID_MTARGET_GOON"])
      return

    self.state.mtarget(player_id, target_id)

  def handle_reveal(self, player_id):
    if not (player_id in self.state.players and self.state.players[player_id].role == "CELEB"):
      self.send_dm(default_resp_lib["INVALID_REVEAL_PLAYER"],player_id)
      return

    if not self.state.phase == MPhase.DAY:
      self.send_dm(default_resp_lib["INVALID_REVEAL_PHASE"],player_id)
      return

    self.state.reveal(player_id)

  def handle_timer(self, player_id):
    self.main_cast("Timer not implemented yet")
  
  def handle_untimer(self, player_id):
    self.main_cast("Timer not implemented yet")

  def handle_main_help(self, text):
    self.main_cast("Help not implemented yet")

  def handle_mafia_help(self, text):
    self.mafia_cast("Help not implemented yet")
    
  def handle_dm_help(self, player_id, text):
    self.send_dm("Help not implemented yet",player_id)

  def handle_main_status(self):
    msg = self.state.main_status()
    self.main_cast(msg)

  def handle_mafia_status(self):
    msg = self.state.mafia_status()
    self.mafia_cast(msg)

  def handle_dm_status(self, player_id):
    msg = self.state.dm_status(player_id)
    self.send_dm(msg,player_id)

  def handle_rule(self, sender, text):
    """ Return the rule for a specific rule or list of rules """
    msg = ""
    words = text.split()
    if len(words) == 1:
      msg = self.rules.describe(has_expl=False)
    elif words[1] in RULE_BOOK:
      rule = words[1]
      msg = "{}:\n".format(rule)
      msg += self.rules.explRule(rule, self.rules[rule])
    elif words[1] == "long":
      msg = self.rules.describe(has_expl=True)
    
    if sender == "MAIN":
      self.main_cast(msg)
    elif sender == "MAFIA":
      self.mafia_cast(msg)
    else:
      self.send_dm(msg, sender)

def createStartRoles(ids, roles, contracts):
  msg = "Roles:"
  players = list(zip(ids,roles))
  for role in ALL_ROLES:
    players_role = [(id,r) for (id,r) in players if r == role]
    for (p_id,r) in players_role:
      msg += "\n  [{}]: {}".format(p_id, r)
      if p_id in contracts:
        msg += " ([{}])".format(contracts[p_id][0])
  return msg