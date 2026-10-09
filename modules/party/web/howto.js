// Party Games: the how-to-play card shown before each game (the first time it's played in a
// night). Shared by the big screen and the phones; keep it in step with the rules.
(() => {
  'use strict';
  window.PartyHowTo = {
    quip: {
      steps: [
        'Answer two silly prompts on your phone. Each one goes up against a friend.',
        'Everyone else votes for the funnier answer.',
        'Final round: everyone answers the same prompt and gets 3 votes to spread around.',
      ],
      score:
        'Matchup points split by the votes, plus bonuses for winning and for a clean sweep. Round 2 counts double.',
    },
    bluff: {
      steps: [
        'A weird true fact appears with a blank. Write a fake answer that sounds real.',
        "Pick the real answer from everyone's lies. You can't pick your own.",
        'Tap ♥ on the lies you love.',
      ],
      score:
        'Points for finding the truth, and for every friend your lie fools. Later questions are worth more.',
    },
    shirt: {
      steps: [
        'Draw designs and write slogans. They all go into one big pile.',
        "Build a shirt from a hand of other people's pieces.",
        'Shirts battle two at a time. The winner stays on until it loses.',
      ],
      score:
        "Votes pay the shirt's artist and slogan writer, with bonuses for winning streaks and the final.",
    },
    drama: {
      steps: [
        'Pitch a theme and vote. Then draw a character in four moods: neutral, flustered, sad, angry.',
        'Write a scene for two characters you only know by name. Someone else writes the twist.',
        'Nobody sees the characters until the big screen plays every scene!',
      ],
      score:
        'Votes for best scene pay its writers and artists. Votes for best character pay the artist.',
    },
  };
})();
