//
//  PutioTableView.m
//  put.io adder
//
//  Created by Nico on 01/11/14.
//  Copyright (c) 2014 Nicolas Oelgart. All rights reserved.
//

#import "PutioTableView.h"
#import "PutioHelper.h"
#import "PutioMainController.h"
#import "NSString+DisplayName.h"

@implementation PutioTableView

- (NSMenu*)menuForEvent:(NSEvent*)event
{
    if (event.type == NSRightMouseDown) {
        if (self.selectedColumn == 0 || self.selectedColumn == 1) {
            return nil;
        } else {
            PutioMainController *controller = [PutioHelper sharedHelper].putioController;
            NSPoint mousePoint = [self convertPoint:[event locationInWindow] fromView:nil];
            long row = [self rowAtPoint:mousePoint];
            PKTransfer *transfer;
            
            @try {
                transfer = [controller.transfers objectAtIndex: row];
            } @catch (NSException *e) {
                return nil;
            } @finally {}
            
            NSDictionary *attributes = @{
               NSFontAttributeName: [NSFont fontWithName:@"Montserrat-Regular" size:10],
               NSForegroundColorAttributeName: [NSColor blackColor]
            };
            
            NSMenu *menu = [[NSMenu alloc] initWithTitle:@"Custom"];
            menu.delegate = controller;
            
            NSString *deleteString = [NSString stringWithFormat:@"Cancel \"%@\"", transfer.name];
            NSMenuItem *deleteItem = [[NSMenuItem alloc] initWithTitle:deleteString action:@selector(cancelTransfer:) keyEquivalent:@""];
            deleteItem.image = [NSImage imageNamed:@"cancel"];
            NSAttributedString *attributedTitle = [[NSAttributedString alloc] initWithString:[deleteItem title] attributes:attributes];
            
            [deleteItem setAttributedTitle:attributedTitle];
            deleteItem.tag = row;
            [menu addItem:deleteItem];
            
            NSString *streamString = [NSString stringWithFormat:@"Stream \"%@\"", transfer.name];
            NSMenuItem *streamItem = [[NSMenuItem alloc] initWithTitle:streamString action:@selector(streamVideo:) keyEquivalent:@""];
            streamItem.image = [NSImage imageNamed:@"vlc"];
            attributedTitle = [[NSAttributedString alloc] initWithString:[streamItem title] attributes:attributes];
            
            [streamItem setAttributedTitle:attributedTitle];
            streamItem.tag = row;
            [menu addItem:streamItem];
            
            return menu;
        }
    }
    
    return nil;
}

@end
