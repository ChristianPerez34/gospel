# Product

## Register

product

## Users

Solo developers working alone at odd hours, deep in a codebase. They think in terminals and diffs. Their hands are on a keyboard, rarely a mouse. They use Gospel for extended focus sessions, sometimes hours at a stretch, often in low ambient light. They are fluent in their tools and impatient with anything that slows them down. They want the agent to feel like a capable pair of hands, not a conversational partner that needs coaxing.

## Product Purpose

Gospel is a desktop agent command center for a developer working across projects. It organizes work into workspaces (project directories) and tasks backed by sessions (conversation threads with context). Starting a task feels like starting a chat: choose a workspace, then send a message. The title is generated from the conversation. The developer monitors concurrent activity from an overview, then opens a task to review code changes, terminal output, and file edits. Success means the developer thinks about the code, not the interface. The tool disappears into the task.

## Brand Personality

Precise, quiet, trusted.

- **Precise**: Every element is intentional. No decoration between intent and result. Tight typography, measured spacing, information-dense but never cluttered.
- **Quiet**: The interface recedes. Dark surfaces, restrained animation, no celebration screens. Status is shown, never announced. The agent works silently and speaks when it has something to say.
- **Trusted**: Reliable and predictable. Same patterns everywhere. No surprises in the UI. The developer trusts the tool because it does exactly what it says, every time.

## Anti-references

- SaaS cream palettes (warm whites, soft shadows, pastoral product illustrations)
- Marketing-style dashboards with hero metrics (big numbers, small labels, gradient accents)
- Chatbot-in-a-box UIs that feel detached from code (floating chat widgets, consumer-messaging skins with dev content pasted in)

## Design Principles

1. **Disappear into the task.** The best interface is one the developer forgets is there. Minimize chrome, maximize content area, let the code be the visual center.
2. **Earn every pixel of brightness.** Dark surfaces are the default because the scene demands it, not because dark looks cool. Every accent color does real work; none is decorative.
3. **Same patterns, same places.** Consistency is an affordance. If the session drawer is on the left, it is always on the left. If the send button is bottom-right, it is always bottom-right. Familiarity IS the feature.
4. **Show, don't tell.** Agent actions appear as inline diffs and terminal output, not status messages. State is visible in the top bar at a glance. Error states are inline and recoverable, never modal roadblocks.
5. **Expert confidence.** No onboarding tours, no tooltips on first visit, no hand-holding. The interface trusts the developer to be smart. Progressive disclosure for power features, not for basics.

## Accessibility & Inclusion

- WCAG AA minimum across all surfaces. AAA for primary text on backgrounds.
- All interactive elements keyboard-reachable with visible focus rings.
- Color never sole indicator of state; always paired with icon or label.
- `prefers-reduced-motion` respected: all animations and transitions set to 0ms.
- Screen reader support for the chat stream via ARIA live regions.
